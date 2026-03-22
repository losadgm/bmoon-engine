use std::rc::Rc;

use caches::{Cache, WTinyLFUCache, WTinyLFUCacheBuilder};
use mlua::prelude::*;

const WINDOW_RATIO: f64 = 0.01;
const PROTECTED_RATIO: f64 = 0.80;

// ── CacheConfig ───────────────────────────────────────────────────────────────

/// Configuración de los tres segmentos internos de W-TinyLFU y su filtro de
/// admisión.
///
/// W-TinyLFU divide la capacidad en tres zonas con propósitos distintos:
///
/// - **Window** — admite entradas nuevas sin pasar por el filtro de frecuencia.
///   Evita el problema de cold-start: una entrada reciente pero poco vista no
///   es expulsada inmediatamente por entradas antiguas con alta frecuencia
///   acumulada.
/// - **Probationary** — zona intermedia. Las entradas promovidas desde Window
///   llegan aquí y compiten con las ya existentes mediante el sketch de
///   frecuencia (TinyLFU).
/// - **Protected** — scripts calientes de alta frecuencia de acceso. Las
///   entradas permanecen aquí mientras sigan siendo accedidas con regularidad.
///
/// El `frequency sketch` (filtro Bloom + contador de saturación) estima la
/// frecuencia de acceso de cualquier clave sin almacenarla explícitamente,
/// usando memoria acotada por `samples`. `false_positive_ratio` controla la
/// precisión del filtro a costa de memoria.
pub struct CacheConfig {
    /// Capacidad de la zona Window (entradas nuevas sin filtro).
    pub window: usize,
    /// Capacidad de la zona Protected (entradas calientes).
    pub protected: usize,
    /// Capacidad de la zona Probationary (candidatos en prueba).
    pub probationary: usize,
    /// Tamaño de la ventana del frequency sketch. Idealmente igual a la
    /// capacidad total — un valor menor reduce memoria a costa de precisión.
    pub samples: usize,
    /// Tasa de falsos positivos del filtro Bloom interno. Debe estar en (0, 1).
    /// Valores menores aumentan la precisión pero también el uso de memoria.
    pub false_positive_ratio: f64,
}

impl CacheConfig {
    /// Construye una configuración con las proporciones por defecto de
    /// W-TinyLFU: Window = 1 %, Protected = 80 %, Probationary = 19 %.
    ///
    /// Estas proporciones son las recomendadas por el paper original y las que
    /// usa `WTinyLFUCache::new()` internamente. Son adecuadas para la mayoría
    /// de perfiles de acceso de scripts TCG, donde un subconjunto pequeño de
    /// cartas es accedido con mucha mayor frecuencia que el resto.
    ///
    /// # Panics
    ///
    /// Panics si `total < 3` (mínimo un slot por segmento).
    pub fn from_capacity(total: usize) -> Self {
        assert!(total >= 3, "total must be >= 3");
        let window = ((total as f64) * WINDOW_RATIO) as usize;
        let protected = ((total as f64) * PROTECTED_RATIO) as usize;
        let probationary = total.saturating_sub(window + protected);
        Self {
            window,
            protected,
            probationary,
            samples: total,
            false_positive_ratio: 0.01,
        }
    }

    /// Construye una configuración con distribución manual de segmentos.
    ///
    /// Útil cuando el perfil de acceso real es conocido de antemano — por
    /// ejemplo, si se sabe que el juego tiene un pool reducido de cartas muy
    /// activas y un largo de cartas raramente usadas, se puede ajustar
    /// `protected` a la baja y `probationary` al alza.
    ///
    /// # Panics
    ///
    /// Panics si cualquier segmento es 0 o si `false_positive_ratio` no está
    /// en el intervalo abierto (0, 1).
    pub fn custom(
        window: usize,
        protected: usize,
        probationary: usize,
        false_positive_ratio: f64,
    ) -> Self {
        assert!(
            window > 0 && protected > 0 && probationary > 0,
            "all segments must be > 0"
        );
        assert!(
            (0.0..1.0).contains(&false_positive_ratio),
            "false_positive_ratio must be in (0, 1)"
        );
        let samples = window + protected + probationary;
        Self {
            window,
            protected,
            probationary,
            samples,
            false_positive_ratio,
        }
    }

    /// Capacidad total efectiva (suma de los tres segmentos).
    pub fn total(&self) -> usize {
        self.window + self.protected + self.probationary
    }
}

// ── LuaTableCache ─────────────────────────────────────────────────────────────

/// Caché de entornos de script, indexado por `ScriptId` (`Rc<str>`).
///
/// Cada entrada es la tabla Luau que resulta de ejecutar el scope global de un
/// script — el entorno sandboxado que contiene sus funciones y
/// sub-tablas de datos. Almacenar esta tabla evita reejecutar el scope global
/// en cada acceso.
///
/// Usa W-TinyLFU como política de evicción, que combina recencia (LRU en la
/// zona Window) y frecuencia (TinyLFU en la zona principal). Es superior a LRU
/// puro para workloads con acceso sesgado — exactamente el caso de un TCG
/// donde un subconjunto pequeño de cartas domina la partida.
///
/// La clave es `Rc<str>` en lugar de `String` para evitar una copia del string
/// en cada inserción. `WTinyLFUCache` acepta `Rc<str>` sin exigir `Send`, lo
/// que es coherente con el diseño monohilo del motor.
pub struct LuaTableCache {
    inner: WTinyLFUCache<Rc<str>, LuaTable>,
}

impl LuaTableCache {
    /// Construye el caché con la configuración dada.
    ///
    /// # Errors
    ///
    /// Devuelve `LuaError` si `WTinyLFUCacheBuilder` rechaza la configuración
    /// (segmentos inconsistentes, `false_positive_ratio` fuera de rango, etc.).
    pub fn new(cfg: CacheConfig) -> LuaResult<Self> {
        let inner =
            WTinyLFUCacheBuilder::new(cfg.window, cfg.protected, cfg.probationary, cfg.samples)
                .set_false_positive_ratio(cfg.false_positive_ratio)
                .finalize()
                .map_err(|e| LuaError::runtime(format!("WTinyLFU init: {:?}", e)))?;
        Ok(Self { inner })
    }

    /// Construye el caché con las proporciones por defecto de W-TinyLFU a
    /// partir de una capacidad total. Equivale a `new(CacheConfig::from_capacity(total))`.
    pub fn new_from_capacity(total: usize) -> LuaResult<Self> {
        Self::new(CacheConfig::from_capacity(total))
    }

    /// Devuelve el entorno del script si está en caché, actualizando su
    /// frecuencia en el sketch. Un hit puede promover la entrada de
    /// Probationary a Protected si su frecuencia es suficientemente alta.
    ///
    /// Devuelve `None` en caso de miss — el llamador debe cargar el script y
    /// llamar a `put`.
    #[inline]
    pub fn get(&mut self, id: &str) -> Option<LuaTable> {
        self.inner.get(id).cloned()
    }

    /// Consulta la caché sin efectos secundarios — no actualiza el frequency
    /// sketch ni el orden LRU. Útil para inspección o debug sin alterar el
    /// comportamiento de evicción.
    #[inline]
    pub fn peek(&self, id: &str) -> Option<LuaTable> {
        self.inner.peek(id).cloned()
    }

    /// Inserta el entorno de un script recién ejecutado.
    ///
    /// Si la caché está llena, TinyLFU compara la frecuencia estimada de la
    /// nueva entrada con el candidato a evictar — solo admite la entrada si su
    /// frecuencia es mayor. Esto protege las entradas calientes frente a
    /// ráfagas de scripts poco frecuentes.
    #[inline]
    pub fn put(&mut self, id: Rc<str>, module: LuaTable) {
        self.inner.put(id, module);
    }

    /// Elimina `id` de la caché. Usado en hot-reload e invalidación por
    /// conflicto de registro. El script será reejecutado en el próximo acceso.
    #[inline]
    pub fn remove(&mut self, id: &str) -> Option<LuaTable> {
        self.inner.remove(id)
    }

    /// Vacía la caché completamente y resetea el frequency sketch. Todos los
    /// scripts serán reejecutados en su próximo acceso.
    #[inline]
    pub fn purge(&mut self) {
        self.inner.purge();
    }

    /// Devuelve `true` si `id` tiene una entrada en caché.
    #[inline]
    pub fn contains(&self, id: &str) -> bool {
        self.inner.contains(id)
    }

    // ── Stats ─────────────────────────────────────────────────────────────────

    /// Número de entradas actualmente en caché (Window + Probationary + Protected).
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Capacidad total configurada.
    pub fn cap(&self) -> usize {
        self.inner.cap()
    }

    /// `(window_len, main_len)` — entradas actuales en Window vs zona
    /// principal (Probationary + Protected). Útil para verificar que la
    /// distribución real de acceso coincide con la configurada.
    pub fn segment_lens(&self) -> (usize, usize) {
        (self.inner.window_cache_len(), self.inner.main_cache_len())
    }

    /// `(window_cap, main_cap)` — capacidades configuradas de Window y zona
    /// principal. Permite verificar en runtime que `CacheConfig` se aplicó
    /// correctamente.
    pub fn segment_caps(&self) -> (usize, usize) {
        (self.inner.window_cache_cap(), self.inner.main_cache_cap())
    }
}
