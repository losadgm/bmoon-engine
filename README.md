# BMoonEngine — Documento de Arquitectura

> **Estado:** En progreso · **Última actualización:** Marzo 2026
> **Autor:** Sesiones de diseño del motor (Rust + Luau)
> **Propósito:** Referencia de decisiones de diseño, incorporación de colaboradores y agentes de IA
>
> ⚠️ **Nota sobre los snippets de código:** todos los ejemplos de código en este documento son prototipos ilustrativos. Su propósito es comunicar intención de diseño, no describir fielmente la implementación final.

---

## Índice

1. [Visión general](#1-visión-general)
2. [Contexto para nuevos colaboradores y agentes](#2-contexto-para-nuevos-colaboradores-y-agentes)
3. [Tecnologías y dependencias](#3-tecnologías-y-dependencias)
4. [Estructura del proyecto](#4-estructura-del-proyecto)
5. [ECS — Entity Component System](#5-ecs--entity-component-system)
   - 5.1 [Por qué ECS](#51-por-qué-ecs)
   - 5.2 [Entidad = la carta física](#52-entidad--la-carta-física)
   - 5.3 [Ejemplo de ciclo de vida de los componentes por zona](#53-ejemplo-de-ciclo-de-vida-de-los-componentes-por-zona)
   - 5.4 [Doble identidad — Carta vs Objeto (Regla 400.7)](#54-doble-identidad--carta-vs-objeto-regla-4007)
   - 5.5 [Procedimiento de cambio de zona (prototipo ilustrativo)](#55-procedimiento-de-cambio-de-zona-prototipo-ilustrativo)
   - 5.6 [Sistemas Rust vs scripts Luau](#56-sistemas-rust-vs-scripts-luau)
6. [Modelo de scripting Luau](#6-modelo-de-scripting-luau)
   - 6.1 [Estructura de un script](#61-estructura-de-un-script)
   - 6.2 [Modularidad de scripts](#62-modularidad-de-scripts)
   - 6.3 [Procedures](#63-procedures-when_)
   - 6.4 [Sistema de triggers y eventos — dos capas](#64-sistema-de-triggers-y-eventos--dos-capas)
   - 6.5 [Desuscripción](#65-desuscripción)
   - 6.6 [Procedures vs Triggers vs Events raw](#66-procedures-vs-triggers-vs-events-raw)
   - 6.7 [Queries ECS desde Luau](#67-queries-ecs-desde-luau)
   - 6.8 [Modelo de suscripción — Lua-driven vs System-driven](#68-modelo-de-suscripción--lua-driven-vs-system-driven)
7. [LuaScriptManager](#7-luascriptmanager)
   - 7.1 [Propósito](#71-propósito)
   - 7.2 [Responsabilidades](#72-responsabilidades)
   - 7.3 [Estructura interna](#73-estructura-interna)
   - 7.4 [API](#74-api)
   - 7.5 [CallResult](#75-callresult)
   - 7.6 [Gestión de errores](#76-gestión-de-errores)
   - 7.7 [Scripting API Layer](#77-scripting-api-layer)
8. [Sistema de eventos](#8-sistema-de-eventos)
   - 8.1 [Arquitectura](#81-arquitectura)
   - 8.2 [API del EventBus](#82-api-del-eventbus)
   - 8.3 [Flujos de comunicación](#83-flujos-de-comunicación)
   - 8.4 [Helper de dispatch compartido](#84-helper-de-dispatch-compartido)
9. [Control del modder — Brainstorming](#9-control-del-modder--brainstorming)
   - 9.1 [Corrutinas — flujos que esperan decisiones](#91-corrutinas--flujos-que-esperan-decisiones)
   - 9.2 [Chain of Responsibility — efectos de reemplazo](#92-chain-of-responsibility--efectos-de-reemplazo)
   - 9.3 [Builder — efectos compuestos legibles](#93-builder--efectos-compuestos-legibles)
   - 9.4 [Middleware / Pipeline — modificar sistemas completos](#94-middleware--pipeline--modificar-sistemas-completos)
   - 9.5 [Event Sourcing ligero — intenciones validadas](#95-event-sourcing-ligero--intenciones-validadas)
   - 9.6 [Composición de patrones](#96-composición-de-patrones)
10. [Patrones de diseño en uso](#10-patrones-de-diseño-en-uso)
11. [Preguntas abiertas / Decisiones futuras](#11-preguntas-abiertas--decisiones-futuras)
12. [Glosario](#12-glosario)

---

## 1. Visión general

Este documento recoge las decisiones arquitectónicas principales de BMoonEngine, un motor de juego de cartas coleccionables (TCG) escrito en **Rust**, con el comportamiento de las cartas definido en scripts **Luau**.

Los cuatro pilares de la arquitectura son:

1. **ECS (Entity Component System)** — modela todas las entidades del juego como datos
2. **Scripting en Luau** — cada carta es un script que define datos estáticos y comportamiento reactivo
3. **Sistema de eventos** — bus de eventos bidireccional Rust ↔ Luau
4. **LuaScriptManager** — módulo autocontenido que actúa de frontera entre Rust y Luau

---

## 2. Contexto para nuevos colaboradores y agentes

Esta sección existe para que cualquier colaborador o agente de IA que se incorpore al proyecto pueda entender rápidamente el estado actual y las decisiones tomadas sin necesidad de leer el documento completo de arriba a abajo.

### Estado actual

Los crates `lua_table`, `lua_table_derive`, `lua_script_manager` y `engine_lua` están implementados. Los tests de integración del manager pasan. El workspace de Rust está configurado. La siguiente tarea es implementar el `engine` (§5 — ECS, sistemas, integración con el manager).

### Decisiones clave ya cerradas

**Sobre el motor:**
- Lenguaje: Rust. Es el primer proyecto serio del autor en Rust — el código debe ser idiomático y legible.
- Scripting: Luau (superset de Lua con tipado opcional y sandboxing). Compatible con la API de Lua estándar.
- El motor es monohilo. No usar `Arc`, `Send` ni `Sync` donde no sea necesario — preferir `Rc`.

**Sobre el LuaScriptManager:**
- Es un módulo completamente desacoplado del engine. No conoce ningún tipo del engine (`EntityId`, `Zone`, etc.).
- Todos los adaptadores de tipo Rust↔Luau viven exclusivamente en el manager.
- El engine construye el DTO `ctx` y se lo pasa al manager — el manager lo traduce a Luau sin conocer su estructura.
- Los scripts no están vinculados a entidades en tiempo de ejecución. La fuente de un efecto viaja en `ctx`, no en el manager.
- La carga del scope global de un script ocurre exactamente una vez — las cargas sucesivas devuelven la tabla cacheada.
- El caché usa `WTinyLFUCache` (crate `caches`) envuelto en un tipo propio `LuaTableCache` con `CacheConfig` configurable.
- Cada script se ejecuta en un entorno sandboxado (tabla propia con metatable `__index → globals`), aislando el estado por script sin perder acceso a los globals del VM.
- `ScriptId` es `Rc<str>` — string interned con refcount sin overhead atómico. `WTinyLFUCache` acepta `Rc<str>` como clave sin exigir `Send`.
- `ScriptMeta` incluye un campo `version: u64` para invalidación de caché en hot-reload.
- `build_registry` es un método separado de `new` y puede llamarse múltiples veces (hot-reload).
- La Scripting API Layer se implementa mediante dos traits: `LuaApiModule` (módulos sin estado) y `LuaContextModule` (módulos con estado compartido vía `Rc<RefCell<T>>` en app data). Cada módulo define su propio namespace global — no existe tabla `engine` intermediaria.

**Sobre el EventBus:**
- La API de `off` es asimétrica: desde Rust se desuscribe por `HandlerId` devuelto por `on`; desde Luau se desuscribe por referencia a la función.
- `RustHandler` es `Rc<dyn Fn(&Lua, LuaMultiValue) -> LuaResult<()>>` — `Rc` porque el motor es monohilo y un mismo handler puede suscribirse a múltiples eventos.
- El bus soporta los cuatro flujos: Rust→Luau, Luau→Rust, Luau→Luau, Rust→Rust.
- `HandlerId` se genera mediante un `AtomicU64` estático (`next_handler_id()`) — no es un campo del `EventBus`. Los IDs son únicos globalmente entre todas las instancias del bus.
- El `EventBus` vive en Lua app data como `Rc<RefCell<EventBus>>`, accesible desde los bindings Luau mediante helpers `with_bus_read` / `with_bus_write`.

**Sobre la conversión de tipos:**
- El engine nunca toca mlua directamente.
- Para leer sub-tablas Luau, el engine usa `#[derive(FromLuaTable)]` — una proc-macro propia definida en `lua_table_derive`.
- `FromLuaTable` y `LuaTableValue` viven en un crate propio `lua_table` — sin dependencias externas. `lua_table_derive` depende de él; `lua_script_manager` lo re-exporta al engine.
- `FromLuaTable` convierte directamente `HashMap<String, LuaTableValue>` al struct del engine sin formato intermedio.

**Sobre la sintaxis de scripts:**
- Las sub-tablas de datos de un script se declaran con asignación: `define = { cost = "G", ... }`. La clave del lado izquierdo es el nombre que `get_table` usa para recuperarla. La sintaxis de llamada a función `define { ... }` no funciona porque el resultado se descartaría — no quedaría almacenado en el entorno del script.

**Sobre `engine_lua`:**
- Las implementaciones de `LuaApiModule` y `LuaContextModule` que necesitan acceder a tipos del engine viven en el crate `engine_lua`, no en el engine ni en el manager. Esto evita el ciclo de dependencias `engine → manager → engine`.
- `engine_lua` depende de `engine`, `lua_script_manager` y `mlua`. Es el único crate del workspace que conoce los tres simultáneamente.
- El binario final y los tests de integración del manager dependen de `engine_lua`.

### Lo que está pendiente de implementar (en orden sugerido)

1. ~~`lua_table_derive` — la proc-macro `#[derive(FromLuaTable)]`~~ ✅
2. ~~`lua_script_manager` — parseo de manifest, registro, caché, API pública, EventBus, Scripting API Layer~~ ✅
3. ~~`engine_lua` — adaptadores `LuaApiModule`/`LuaContextModule` y tests de integración del manager~~ ✅
4. `engine` — ECS, sistemas, integración con el manager

---

## 3. Tecnologías y dependencias

### Lenguajes y runtime

| Tecnología | Uso |
|---|---|
| **Rust** | Lenguaje principal del motor |
| **Luau** | Lenguaje de scripting para las cartas (superset de Lua con sandboxing y tipado opcional) |

### Dependencias por crate

#### `lua_table`
```toml
[dependencies]
# sin dependencias externas
```

Crate de tipos compartidos. Define `LuaTableValue` y el trait `FromLuaTable`. No depende de mlua ni de ningún otro crate interno. Es la única fuente de verdad para estos tipos — `lua_table_derive` los referencia en el código que genera; `lua_script_manager` los re-exporta al engine.

#### `lua_table_derive`
```toml
[lib]
proc-macro = true

[dependencies]
syn         = { version = "2", features = ["full"] }
quote       = "1"
proc-macro2 = "1"

[dev-dependencies]
lua_table = { path = "../lua_table" }
```

Crate exclusivo para la proc-macro `#[derive(FromLuaTable)]`. No depende de mlua. Los tests de integración de la macro viven en `lua_table_derive/tests/` y dependen de `lua_table` como `dev-dependency`.

#### `lua_script_manager`
```toml
[dependencies]
lua_table        = { path = "../lua_table" }
lua_table_derive = { path = "../lua_table_derive" }
mlua             = { version = "0.11.6", features = ["luau", "vendored"] }
caches           = "0.3.0"
ahash            = "0.8"
serde            = { version = "1", features = ["derive"] }
toml             = "1"
walkdir          = "2"

[dev-dependencies]
lua_table_derive = { path = "../lua_table_derive" }
tempfile         = "3"
```

- `mlua` con features `luau` y `vendored` — embebe el runtime de Luau directamente en el binario, sin dependencia de instalación externa
- `caches` — proporciona `WTinyLFUCache` para el `LuaTableCache`
- `ahash` — hasher de alto rendimiento para los `AHashMap` del `EventBus` y el `ScriptRegistry`
- `toml` + `serde` — deserialización del `manifest.toml` a structs Rust
- `walkdir` — descubrimiento de manifests recorriendo el árbol de `assets/`

#### `engine`
```toml
[dependencies]
lua_script_manager = { path = "../lua_script_manager" }
```

El engine no depende de mlua ni de serde directamente — todo pasa por el manager.

#### `engine_lua`
```toml
[dependencies]
engine             = { path = "../engine" }
lua_script_manager = { path = "../lua_script_manager" }
mlua               = { version = "0.11.6", features = ["luau", "vendored"] }

[dev-dependencies]
lua_table = { path = "../lua_table" }
tempfile  = "3"
```

Crate de pegamento entre el engine y el VM de Luau. Implementa `LuaApiModule` y `LuaContextModule` para los módulos del engine (`GameApi`, `WorldApi`, `EntityApi`, etc.). Es el único crate del workspace que depende simultáneamente de `engine`, `lua_script_manager` y `mlua`. Los tests de integración del manager viven aquí.

---

## 4. Estructura del proyecto

```
bmoon_project/
├── bmoon_engine/
│   ├── Cargo.toml                          ← workspace root
│   └── crates/
│       ├── engine/
│       │   └── Cargo.toml
│       ├── engine_lua/
│       │   ├── Cargo.toml
│       │   ├── src/
│       │   │   └── lib.rs
│       │   └── tests/
│       │       └── manager_tests.rs        ← tests de integración del manager
│       ├── lua_script_manager/
│       │   └── Cargo.toml
│       ├── lua_table/
│       │   └── Cargo.toml
│       └── lua_table_derive/
│           ├── Cargo.toml
│           └── tests/
│               └── derive_tests.rs         ← tests de integración de la macro
└── assets/
    └── my_card_game/                       ← paquete de recursos (uno por juego/expansión)
        ├── manifest.toml                   ← registro de scripts del paquete
        └── scripts/
            └── cards/
                ├── blood_moon.luau
                └── lightning_bolt.luau
```

```toml
# bmoon_engine/Cargo.toml (workspace root)
[workspace]
members = [
    "crates/engine",
    "crates/engine_lua",
    "crates/lua_table",
    "crates/lua_table_derive",
    "crates/lua_script_manager",
]
resolver = "2"
```

### Manifest de un paquete de assets

```toml
# assets/my_card_game/manifest.toml
[package]
name = "my_card_game"

[[script]]
id   = "cards.blood_moon"
path = "scripts/cards/blood_moon.luau"

[[script]]
id   = "cards.lightning_bolt"
path = "scripts/cards/lightning_bolt.luau"
```

Solo se registran los scripts principales. Los módulos auxiliares (`abilities/`, `utils/`) son resueltos automáticamente por `require` de Luau sin necesidad de registro.

---

## 5. ECS — Entity Component System

### 5.1 Por qué ECS

En ECS, una entidad es simplemente un ID entero. Los datos viven en componentes; el comportamiento vive en sistemas que iteran sobre entidades con combinaciones específicas de componentes. Para un TCG este modelo es ideal porque:

- La naturaleza de una carta cambia según ciertas variables como la zona (mano, pila, campo de batalla, cementerio).
- Las alteraciones de estas variables son intercambios de componentes, no destrucción y recreación de objetos
- Las propiedades booleanas (Vuelo, Girado, Indestructible) se convierten en componentes marcadores consultables de forma nativa en Rust sin necesidad de parsing

### 5.2 Entidad = la carta física

Un único `entity_id` representa una carta física a lo largo de todas sus variaciones durante la partida. Esto permite que los efectos de "exiliar y regresar" rastreen el mismo objeto a través de cualquier transición de zona.

```
entity_id: 42 (Llanowar Elves)
  + CardIdentity   { card_id, script_id: "cards.blood_moon", owner: p1 }
  + ObjectVersion  { version: 2 }            ← se incrementa en cada cambio de zona
  + ZoneComponent  { zone: Battlefield }

  -- Componentes exclusivos del campo de batalla (se eliminan al salir):
  + PowerToughness { power: 1, toughness: 1 }
  + Colors         { Green }
  + Types          { types: [Creature], subtypes: [Elf] }
  + Controller     { player_id: 1 }
  + SummoningSickness                         ← componente marcador
```

### 5.3 Ejemplo de ciclo de vida de los componentes por zona

| Componente         | Librería | Mano | Pila  | Campo       | Cementerio | Exilio |
|--------------------|----------|------|-------|-------------|------------|--------|
| CardIdentity       | ✅       | ✅   | ✅    | ✅          | ✅         | ✅     |
| ScriptComponent    | ✅       | ✅   | ✅    | ✅          | ✅         | ✅     |
| ObjectVersion      | ✅+1    | ✅+1 | ✅+1  | ✅+1        | ✅+1       | ✅+1   |
| PowerToughness     | ❌       | ❌   | ❌    | ✅          | ❌         | ❌     |
| Controller         | ❌       | ❌   | ❌    | ✅          | ❌         | ❌     |
| Tapped             | ❌       | ❌   | ❌    | dinámico    | ❌         | ❌     |
| Counters           | ❌       | ❌   | ❌    | dinámico    | ❌         | ❌     |
| SummoningSickness  | ❌       | ❌   | ❌    | al entrar   | ❌         | ❌     |
| SpellComponent     | ❌       | ❌   | ✅    | ❌          | ❌         | ❌     |
| CastableComponent  | ❌       | ✅   | ❌    | ❌          | ❌         | ❌     |

### 5.4 Doble identidad — Carta vs Objeto (Regla 400.7)

Regla Comprehensive de TCG 400.7: *"Un objeto que ha cambiado de zona se convierte en un nuevo objeto sin memoria de su existencia anterior."*

Esto se modela con dos capas:

```rust
struct CardIdentity {
    card_id:   CardId,   // permanente — identifica la carta física
    script_id: ScriptId, // "cards.blood_moon"
    owner:     PlayerId,
}

struct ObjectVersion(u64); // se incrementa en cada cambio de zona
```

**El targeting** almacena `(entity_id, version)`. Cuando el hechizo resuelve, si las versiones difieren → fizzle.

**Las auras** almacenan `(entity_id, version)` del permanente encantado. Las SBA comprueban versiones obsoletas y mueven el Aura al cementerio.

**Los efectos de "exiliar y regresar"** usan solo `entity_id` — no les importa la versión.

### 5.5 Procedimiento de cambio de zona (prototipo ilustrativo)

```rust
fn move_to_zone(world: &mut World, entity: EntityId, to: Zone) {
    // 1. Eliminar componentes de la zona anterior
    match world.get::<ZoneComponent>(entity).current {
        Zone::Battlefield => {
            world.remove::<PowerToughness>(entity);
            world.remove::<Tapped>(entity);
            world.remove::<SummoningSickness>(entity);
            world.remove::<Counters>(entity);
            world.remove::<Controller>(entity);
        }
        Zone::Stack => { world.remove::<SpellComponent>(entity); }
        _ => {}
    }

    // 2. Incrementar ObjectVersion → nuevo objeto
    world.get_mut::<ObjectVersion>(entity).0 += 1;

    // 3. Actualizar zona
    world.get_mut::<ZoneComponent>(entity).current = to;

    // 4. Emitir evento → dispara when_* en el script
    world.emit(ZoneChanged { entity, to });
}
```

### 5.6 Sistemas Rust vs scripts Luau

| | Sistema Rust | Script Luau |
|---|---|---|
| **Alcance** | Itera TODAS las entidades con los componentes dados | Reacciona a eventos de UNA entidad |
| **Propósito** | SBA, cálculo de combate, desgirar, robar | Comportamiento específico de la carta |
| **Puede consultar ECS** | Sí (rol principal) | Sí (via `ctx.world.query()`) |

Los scripts Luau **no son Sistemas** — son hooks reactivos a eventos asociados a una única entidad.

---

## 6. Modelo de scripting Luau

### 6.1 Estructura de un script

Cada carta es un archivo Luau con dos responsabilidades:

```lua
-- ── 1. Sub-tablas top-level → leídas por el engine via get_table() ────────
-- Se declaran como asignaciones para que queden almacenadas en el entorno
-- del script y sean recuperables por nombre via get_table().
define = {
    cost      = "G",
    power     = 1, toughness = 1,
    types     = { "Creature" },
    subtypes  = { "Elf" },
    colors    = { "G" },
    keywords  = { "vigilance" },
}

-- ── 2. Procedures (cambio de zona) ────────────────────────────────────────
-- ctx es inyectado por el engine como parámetro en cada llamada.
-- La fuente asociada al script (entidad, controlador, etc.) viaja en ctx.

function when_enters_battlefield(ctx)
    ctx.entity.add("ManaAbility", { cost = "tap", mana = "G" })
    ctx.game.draw_card(ctx.entity.controller)
    ctx.triggers.on_creature_dies(on_creature_dies)
end

function on_creature_dies(ctx, creature)
    if creature ~= ctx.entity then
        ctx.entity.modify("PowerToughness", function(pt)
            pt.power += 1
        end)
    end
end

function when_leaves_battlefield(ctx)
    -- cleanup si es necesario
end
```

**Por qué `define = { ... }` y no `define { ... }` (llamada a función):** `get_table` recupera sub-tablas por nombre de clave en el entorno del script. La asignación `define = { ... }` almacena la tabla bajo la clave `"define"` en ese entorno. La sintaxis de llamada a función `define { ... }` ejecutaría una función `define` y descartaría el resultado — la tabla no quedaría almacenada en ningún sitio accesible para `get_table`.

**ctx siempre es un parámetro, nunca una variable global ni un upvalue capturado:** `ctx` es construido de nuevo por el engine en cada llamada. Capturar `ctx` de un scope exterior resultaría en un snapshot obsoleto; recibirlo como parámetro siempre refleja el estado actual del juego. La fuente asociada al script (entidad, controlador, zona actual) es accesible exclusivamente a través de `ctx`.

### 6.2 Modularidad de scripts

Una carta tiene exactamente un `ScriptComponent` con un `script_id`. La modularidad se delega al sistema de módulos nativo de Lua mediante `require`, sin romper la unicidad de identidad.

Solo los scripts principales de cartas están registrados en el manifest del manager. Los módulos auxiliares (habilidades compartidas, utilidades) viven en el filesystem y Luau los encuentra via `require` sin que el manager necesite conocerlos:

```lua
-- cards/llanowar_elves.luau  (registrado en el manifest)
local mana_ability = require("abilities.tap_for_mana")
local elf_synergy  = require("abilities.elf_synergy")

define = { cost = "G", power = 1, toughness = 1, types = { "Creature" }, subtypes = { "Elf" } }

function when_enters_battlefield(ctx)
    mana_ability.setup(ctx)
    elf_synergy.setup(ctx)
end
```

```lua
-- abilities/tap_for_mana.luau  (módulo auxiliar, no registrado en el manifest)
local M = {}

function M.setup(ctx)
    local handler = function(ctx)
        ctx.game.add_mana(ctx.entity.controller, "G")
    end
    events.on("on_tap", handler)
end

return M
```

### 6.3 Procedures (when\_\*)

Los procedures son **llamados directamente por el engine** — no son suscripciones opcionales. Mapean directamente a cambios de zona:

| Procedure | Transición de zona | Notas |
|---|---|---|
| `when_cast` | Cualquiera → Pila | Solo para cartas lanzables |
| `when_enters_battlefield` | Pila/Cualquiera → Campo | Instanciación + efectos ETB |
| `when_leaves_battlefield` | Campo → Cualquiera | Limpieza |
| `when_dies` | Campo → Cementerio | Atajo para LTB + Cementerio |
| `when_exiled` | Cualquiera → Exilio | |
| `when_drawn` | Librería → Mano | |
| `when_resolves` | Resolución en pila | Hechizos y habilidades activadas |

Si un script no define un procedure, no se considera un error — el engine aplica el comportamiento por defecto (no-op en la mayoría de casos).

**Por qué basado en cambio de zona y no en acciones:** una carta que entra al campo desde la mano, el cementerio (reanimar), el exilio (suspender) o que se crea como token son el mismo evento — `when_enters_battlefield`. Un modelo basado en acciones requeriría múltiples procedures para un comportamiento idéntico.

### 6.4 Sistema de triggers y eventos — dos capas

Los scripts tienen acceso a dos niveles de interacción con el sistema de eventos:

**Capa declarativa — `ctx.triggers` y sistemas equivalentes**

API de alto nivel implementada por el engine y expuesta a Luau via bindings en `ctx`. Gestiona automáticamente el orden de triggers, la prioridad y las reglas del juego. Es la forma estándar de que una carta reaccione a eventos. `ctx.triggers` es un ejemplo — puede haber otros sistemas del engine expuestos de forma similar:

```lua
function when_enters_battlefield(ctx)
    ctx.triggers.on_creature_dies(on_creature_dies)
    ctx.triggers.on_upkeep(on_upkeep)
end
```

El engine recoge estas intenciones, las ordena según las reglas, y llama al handler en el momento correcto. Los handlers se limpian automáticamente cuando la carta abandona el campo.

**Capa raw — `events` (tabla global)**

Acceso directo al `EventBus` del manager, disponible como tabla global en el VM de Luau. No está ligada a `ctx` ni a ninguna entidad concreta. Pensada para scripts avanzadas o para extender el engine desde Luau:

```lua
local handler = function(ctx, creature)
    -- lógica personalizada
end

events.on("on_creature_dies", handler)
-- más tarde, desuscribir por referencia a la función
events.off("on_creature_dies", handler)
```

El uso de `events` raw implica que el modder asume la responsabilidad del orden de ejecución y la limpieza de handlers. No hay garantías de orden ni integración con las reglas del juego.

### 6.5 Desuscripción

Los handlers registrados via `ctx.triggers` son gestionados automáticamente por el engine — se limpian al salir de la zona correspondiente sin intervención del script.

Los handlers registrados via `events` raw tienen mecanismos de desuscripción distintos según el lado desde el que se opere:

**Desde Luau** — se desuscriben pasando una referencia directa a la función. Las funciones son valores de primera clase en Luau y pueden compararse por referencia, lo que hace cualquier handler desuscribible — tanto funciones con nombre como lambdas anónimas — siempre que se conserve la referencia:

```lua
function when_enters_battlefield(ctx)
    self.handler = function(ctx, creature)
        -- lógica
    end
    events.on("on_creature_dies", self.handler)
end

function when_leaves_battlefield(ctx)
    events.off("on_creature_dies", self.handler)  -- por referencia a la función
end
```

**Desde Rust** — `on` devuelve un `HandlerId` que el caller debe conservar. `off` recibe ese `HandlerId` para localizar y eliminar el handler en O(1):

```rust
let id = manager.on("on_creature_dies", handler)?;
// más tarde
manager.off("on_creature_dies", id)?;
```

El problema de re-entrancy por blinks aplica únicamente a `events` raw. Con `ctx.triggers` el engine lo gestiona:

```
Blink 1 → 1 suscripción   → el evento dispara el handler ×1  ✅
Blink 2 → 2 suscripciones → el evento dispara el handler ×2  ❌
Blink N → N suscripciones → el evento dispara el handler ×N  💥
```

### 6.6 Procedures vs Triggers vs Events raw

| | Procedures `when_*` | `ctx.triggers` | `events` raw |
|---|---|---|---|
| **Nivel** | Engine | Engine (alto nivel) | Manager (bajo nivel) |
| **Dirección** | Engine → Carta | Carta declara intención | Bidireccional |
| **Orden de juego** | N/A | Gestionado por engine | No garantizado |
| **Limpieza** | N/A | Automática | Manual |
| **Audiencia** | Todos | Modders estándar | Modders avanzados |

### 6.7 Queries ECS desde Luau

Los scripts pueden consultar el mundo para encontrar otras entidades:

```lua
ctx.world.query()
    :with_component("Types", "Creature")
    :with_component("Zone", "Battlefield")
    :controlled_by(ctx.entity.controller)
    :exclude(ctx.entity)
    :each(function(entity)
        entity.add("Flying")
    end)
```

### 6.8 Modelo de suscripción — Lua-driven vs System-driven

> ⚠️ Esta sección documenta una **pregunta de diseño sin resolver** que conviene explorar antes de finalizar el modelo de suscripción.

#### Enfoque actual: suscripciones gestionadas desde Luau

Las cartas declaran triggers explícitamente dentro de sus hooks via `ctx.triggers`. Pros: flexible, controlado por la carta, evidente para el modder. Contras: el engine no tiene conocimiento estático de lo que escucha una carta antes de cargarla.

#### Alternativa: auto-suscripciones gestionadas por Sistemas

En lugar de que cada carta declare sus triggers manualmente, los Sistemas Rust podrían registrar automáticamente triggers basándose en los componentes que tiene la entidad. El script solo definiría los handlers. Pros: ciclo de vida gestionado íntegramente por Rust, el engine tiene visibilidad completa de los triggers, encaja con el modelo ECS puro. Contras: menos flexible — las cartas no pueden suscribirse condicionalmente en función del estado del juego en runtime.

#### Óptimo probable: enfoque híbrido

- **Triggers estáticos similares a keywords** (Lifelink, Deathtouch, habilidades disparadas ligadas a un tipo) → gestionados por Sistemas, derivados de componentes marcadores. Sin Luau necesario.
- **Triggers complejos y condicionales** (efectos de cartas que dependen del estado del juego) → gestionados desde Luau, declarados explícitamente dentro de `when_enters_battlefield` via `ctx.triggers`.

```lua
-- Habilidad estática → no necesita trigger, gestionada por CombatSystem leyendo FlyingComponent
define = { keywords = { "flying", "lifelink" } }

-- Trigger condicional complejo → declaración desde Luau
function when_enters_battlefield(ctx)
    ctx.triggers.on_spell_cast(on_spell_cast)
end
```

---

## 7. LuaScriptManager

### 7.1 Propósito

`LuaScriptManager` es un módulo autocontenido y desacoplado del engine que actúa como única frontera entre Rust y Luau. El engine es su único consumidor previsto, pero el manager no conoce ningún tipo del engine (`EntityId`, `Zone`, etc.). Todos los adaptadores de tipo entre Rust y Luau viven exclusivamente en el manager — nunca repartidos por el engine.

### 7.2 Responsabilidades

- **Parseo del manifest** — lectura de `manifest.toml` y construcción del registro de scripts
- **Registro de scripts** — caché de metadatos (`ScriptRegistry`) por `script_id`
- **Carga y caché de tablas** — ejecución lazy o explícita de scripts y almacenamiento en `LuaTableCache` (wrapper tipado y preconfigurado de `WTinyLFUCache`) por `script_id`
- **Soporte de módulos** — resolución de `require` para módulos auxiliares no registrados en el manifest
- **Scripting API Layer** — capa de exposición estructurada en cinco fases que define cómo el engine se comunica con los scripts (ver §7.7)

**Fuera del alcance:**
- **Conocimiento de tipos del engine** — todos los tipos concretos del engine son opacos para el manager
- **Reglas del juego** — el orden de triggers, la prioridad y la pila son responsabilidad del engine

### 7.3 Estructura interna

#### Manifest

El manifest describe los scripts principales del proyecto. Los módulos auxiliares no se registran — son resueltos automáticamente por `require`:

```toml
[package]
name = "my_card_game"

[[script]]
id   = "cards.blood_moon"
path = "scripts/cards/llanowar_elves.luau"

[[script]]
id   = "cards.lightning_bolt"
path = "scripts/cards/lightning_bolt.luau"
```

#### Registro y caché

```rust
// Metadatos por script_id, cargados al inicializar el manager
ScriptRegistry: AHashMap<ScriptId, ScriptMeta>

// ScriptId es Rc<str> — string interned con refcount, sin overhead atómico
type ScriptId = Rc<str>;

struct ScriptMeta {
    path:    PathBuf,
    version: u64,   // se incrementa en hot-reload para invalidar el caché
}

// LuaTable por script_id — wrapper tipado y preconfigurado de WTinyLFUCache
LuaTableCache: WTinyLFUCache<ScriptId, LuaTable>
```

El scope global de cada script se ejecuta exactamente una vez — cuando se carga por primera vez. Las cargas sucesivas devuelven la tabla cacheada sin reejecutar el script. El caché persiste durante toda la partida y es limpiado automáticamente por Rust al finalizar.

### 7.4 API

#### Carga explícita

```rust
fn load(script_id: &ScriptId) -> Result<(), ScriptLoadError>
```

Ejecuta el script y almacena la `LuaTable` resultante en `LuaTableCache`. No-op si ya está cargada. Si el script no existe en el registro, loggea el error — el engine es responsable de aplicar la carta fallback.

#### Ejecución de funciones

```rust
fn call<T: FromLua>(script_id: &ScriptId, fn_name: &str, ctx: impl IntoLuaMulti) -> CallResult<T>
```

Llama a una función del script. Lazy load si la tabla no está en caché. El engine construye el DTO `ctx` y es responsable de su contenido — el manager lo traduce a Luau sin conocer su estructura. `fn_name` es determinado exclusivamente por el engine; el manager es agnóstico a su significado.

#### Lectura de sub-tablas

```rust
fn get_table<T: FromLuaTable>(script_id: &ScriptId, table_name: &str) -> Option<T>
```

Recupera una sub-tabla de la `LuaTable` cacheada por nombre y la convierte directamente a `T`. Lazy load si la tabla no está en caché. Devuelve `None` tanto si la sub-tabla no existe como si la conversión falla — en ambos casos el engine aplica fallback. El manager loggea internamente el motivo si fue un error de conversión.

El engine no tiene contacto con `LuaTable` ni con ningún tipo de mlua. La conversión es directa — sin formato intermedio — gracias a un trait propio del manager con una proc-macro derivable:

```rust
// Trait definido en el manager
trait FromLuaTable: Sized {
    fn from_lua_table(map: HashMap<String, LuaTableValue>) -> Result<Self, String>;
}

// Tipos primitivos que puede contener una sub-tabla Luau
enum LuaTableValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<LuaTableValue>),
    Map(HashMap<String, LuaTableValue>),  // para tablas anidadas
}
```

El engine deriva la implementación automáticamente — sin lógica manual de conversión y sin dependencia en mlua:

```rust
// En el engine — solo un derive
#[derive(FromLuaTable)]
struct CardDefinition {
    cost:     String,
    power:    Option<i64>,
    keywords: Vec<String>,
}

// Uso desde el engine — idéntico para cualquier sub-tabla
manager.get_table::<CardDefinition>(script_id, "define")
manager.get_table::<SomeOtherData>(script_id, "metadata")
```

La proc-macro `#[derive(FromLuaTable)]` genera la implementación de `FromLuaTable` en tiempo de compilación, inspeccionando los campos del struct y generando las conversiones desde `LuaTableValue` correspondientes. Esto elimina tanto el boilerplate manual como cualquier formato de serialización intermedio.

> ⚠️ La proc-macro es el componente de mayor coste de implementación del manager. Se puede diferir usando una implementación manual de `FromLuaTable` en los primeros structs del engine, e introducir el derive cuando el número de tipos lo justifique.

#### Dispatch de eventos

```rust
fn emit(event_name: &str, args: impl IntoLuaMulti)
```

Despacha síncronamente a todos los handlers suscritos al evento — tanto Luau como Rust. Ver §8 para la API completa del EventBus.

### 7.5 CallResult

```rust
enum CallResult<T> {
    Ok(T),
    FunctionNotFound,         // no-op esperado, el engine aplica su default
    ScriptError(LuaScriptError),
}

enum LuaScriptError {
    RuntimeError(String),     // error en tiempo de ejecución del script
    ConversionError(String),  // el valor de retorno no se pudo convertir a T
}
```

El manager loggea internamente cualquier `ScriptError` antes de devolverlo — el engine nunca necesita loggear en el call site. El engine elige el nivel de detalle según la criticidad del contexto:

```rust
// Máximo detalle — casos críticos
match manager.call::<T>(script_id, fn_name, ctx) {
    CallResult::Ok(value)        => ...,
    CallResult::FunctionNotFound => apply_default(),
    CallResult::ScriptError(_)   => apply_critical_fallback(),
}

// Solo el valor si existe — casos no críticos
let value = manager.call::<T>(script_id, fn_name, ctx)
    .into()          // via From<CallResult<T>> for Option<T>
    .unwrap_or(default);
```

### 7.6 Gestión de errores

Hay dos categorías de error con ciclos de vida distintos:

**Errores Luau** (sintaxis, semántica, mal uso de la API de scripts) → responsabilidad del manager. Se capturan, se loggean con contexto completo (`script_id`, nombre de función, línea del error) y se devuelven como `CallResult::ScriptError`. El manager nunca hace panic.

**Errores del engine** provocados por scripts que alteran el estado interno del engine → el engine los detecta y loggea por su cuenta. El manager no tiene visibilidad ni responsabilidad sobre ellos.

En ambos casos, si el error no deja el sistema en un estado irrecuperable, el engine aplica fallbacks y continúa la partida.

### 7.7 Scripting API Layer

La capa de scripting define cómo el engine expone funcionalidad a los scripts Luau. Se estructura en cinco fases ordenadas de menor a mayor acoplamiento:

**Fase 1 — API Global (funciones libres)**
Funciones registradas directamente en el entorno global del VM de Luau. Cada módulo define su propio namespace y se registra como global directamente — no existe tabla `engine` intermediaria. Los scripts acceden a los módulos por su nombre de namespace: `events.on(...)`, `game.draw_card(...)`, `world.query(...)`.

La Scripting API Layer se implementa mediante dos traits:

```rust
/// Módulo sin estado — registra funciones en una tabla global.
trait LuaApiModule {
    fn namespace() -> &'static str;
    fn register(lua: &Lua, table: &LuaTable) -> LuaResult<()>;
}

/// Módulo con estado — envuelve el contexto en Rc<RefCell<T>>,
/// lo guarda en Lua app data y expone funciones que lo acceden
/// via ctx_read / ctx_write.
trait LuaContextModule {
    type Context: 'static;
    fn namespace() -> &'static str;
    fn bind(lua: &Lua, table: &LuaTable) -> LuaResult<()>;
}
```

El engine registra módulos al inicializar el manager:

```rust
manager.register_api::<MyMathApi>()?;                    // sin estado
manager.bind_context::<GameApi>(GameState::default())?;  // con estado
```

La tabla `events` es el ejemplo principal de esta fase — disponible en cualquier script sin necesidad de `ctx`.

**Fase 2 — UserData (tipos Rust como objetos Luau)**
Structs del engine expuestos a Luau como objetos con métodos. El engine controla explícitamente qué métodos son visibles y cuáles permiten mutación. El manager gestiona el ciclo de vida de estos objetos dentro del VM sin conocer su tipo concreto. Permite que los scripts interactúen con estructuras complejas del engine de forma segura y ergonómica.

**Fase 3 — Contexto de Engine (paso de estado — bindings)**
El engine construye un DTO `ctx` en cada llamada a un script e inyecta en él los bindings que ese script necesita — namespaces como `ctx.game`, `ctx.world`, `ctx.entity` o `ctx.triggers`. El manager inyecta `ctx` como parámetro sin conocer su estructura. Esta fase permite que el engine controle con precisión qué estado y qué operaciones están disponibles en cada llamada concreta.

**Fase 4 — Sistema de Eventos Bidireccional**
El `EventBus` del manager como canal de comunicación completo entre Rust y Luau. Soporta los cuatro flujos (Rust→Luau, Luau→Rust, Luau→Luau, Rust→Rust) y es accesible desde Luau via la tabla global `events` (Fase 1) y desde Rust via el manager. Ver §8 para la API completa.

**Fase 5 — Sandboxing**
Cada script se ejecuta en un entorno sandboxado: una tabla propia con metatable `__index → globals`. Esto aísla el estado global de cada script sin perder acceso a los built-ins y módulos API registrados en el VM. La arquitectura de Luau provee el resto del aislamiento — no requiere implementación adicional en el manager.

---

## 8. Sistema de eventos

### 8.1 Arquitectura

El `EventBus` almacena dos mapas de handlers independientes. El doble mapa por evento e `HandlerId` permite localizar y eliminar cualquier handler en O(1) sin reindexar:

```rust
type RustHandler = Rc<dyn Fn(&Lua, LuaMultiValue) -> LuaResult<()>>;

pub struct EventBus {
    lua_handlers:  AHashMap<String, AHashMap<HandlerId, LuaRegistryKey>>,
    rust_handlers: AHashMap<String, AHashMap<HandlerId, RustHandler>>,
}
```

`HandlerId` se genera mediante una función libre `next_handler_id()` con un `AtomicU64` estático interno. Los IDs son únicos globalmente entre todas las instancias del bus — el `EventBus` no carga con ese estado:

```rust
fn next_handler_id() -> HandlerId {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    HandlerId(COUNTER.fetch_add(1, Ordering::Relaxed))
}
```

El `EventBus` vive en Lua app data como `Rc<RefCell<EventBus>>`. Los bindings Luau lo acceden mediante helpers `with_bus_read` / `with_bus_write` sin necesidad de raw pointers. `Rc` es apropiado porque el motor es monohilo y mlua no exige `Send` cuando la feature `send` está desactivada.

`LuaRegistryKey` mantiene los closures Luau anclados en el registro del VM de Luau, evitando que el GC de Luau los recolecte mientras el handler esté activo. Cuando el `HandlerId` correspondiente se elimina del mapa, el `LuaRegistryKey` se dropea y el GC de Luau puede recolectar el closure.

`RustHandler` usa `Rc` en lugar de `Box` para permitir que el mismo closure Rust sea suscrito a múltiples eventos sin necesidad de clonar el closure — solo se clona el puntero.

### 8.2 API del EventBus

La API es asimétrica por diseño — cada lado usa el mecanismo de desuscripción más natural para su contexto:

```rust
// ── Suscribir ──────────────────────────────────────────────────────────────

// Desde Rust — devuelve un HandlerId para desuscribir después
pub fn on<F>(&self, event: &str, handler: F) -> LuaResult<HandlerId>
    where F: Fn(&Lua, LuaMultiValue) -> LuaResult<()> + 'static

// Desde Luau
on: (event: string, handler: (...any) -> ()) -> ()


// ── Desuscribir un handler concreto de un evento concreto ──────────────────

// Desde Rust — por HandlerId
pub fn off(&self, event: &str, id: HandlerId) -> LuaResult<()>

// Desde Luau — por referencia a la función
off: (event: string, handler: (...any) -> ()) -> ()


// ── Desuscribir todos los handlers de un evento ────────────────────────────

pub fn off_all(&self, event: &str) -> LuaResult<()>

off_all: (event: string) -> ()                          // desde Luau


// ── Emitir ─────────────────────────────────────────────────────────────────

pub fn emit<A: IntoLuaMulti + Clone>(&self, event: &str, args: A) -> LuaResult<()>

emit: (event: string, ...any) -> ()                     // desde Luau
```

**Desde Rust** — `on` devuelve un `HandlerId` que el caller debe conservar para poder desuscribir después. `off` recibe ese `HandlerId` y localiza el handler en O(1) via el doble mapa.

**Desde Luau** — `on` no devuelve nada. `off` recibe una referencia directa a la función — Luau puede comparar referencias a funciones, por lo que cualquier handler es desuscribible siempre que se conserve la referencia, incluidas las lambdas anónimas asignadas a una variable.

El mismo handler puede suscribirse a múltiples eventos. Desde Rust, cada llamada a `on` devuelve un `HandlerId` distinto por evento. Desde Luau, basta con pasar la misma referencia a `on` para cada evento y luego a `off` para desuscribir de uno concreto sin afectar a los demás.

### 8.3 Flujos de comunicación

El `EventBus` es el bus central del juego y soporta los cuatro flujos:

| Flujo | Emisor | Receptor | Notas |
|---|---|---|---|
| Rust → Luau | `manager.emit(...)` | `events.on(...)` en Luau | |
| Luau → Rust | `events.emit(...)` en Luau | `manager.on(...)` | |
| Luau → Luau | `events.emit(...)` en Luau | `events.on(...)` en Luau | |
| Rust → Rust | `manager.emit(...)` | `manager.on(...)` | Útil para el sistema de triggers del engine; las magic strings no tienen las garantías de un observer tipado — usar con criterio |

Cada `emit` — tanto desde Rust como desde Luau — despacha síncronamente a ambos mapas (`lua_handlers` y `rust_handlers`).

### 8.4 Helper de dispatch compartido

Tanto `manager.emit` como `events.emit` comparten el mismo dispatch interno:

```rust
fn dispatch_lua<A: IntoLuaMulti + Clone>(funcs: &[LuaFunction], args: A) -> LuaResult<()> {
    if let [init @ .., last] = funcs {
        for f in init { f.call::<()>(args.clone())?; }
        last.call::<()>(args)?;
    }
    Ok(())
}
```

---

## 9. Control del modder — Brainstorming

> ⚠️ Esta sección es exploratoria. Las ideas aquí recogidas están en fase de brainstorming y requieren refinamiento antes de convertirse en decisiones de arquitectura.

El objetivo es dar al modder control máximo sobre el flujo de ejecución sin exponerle la complejidad interna del engine. Dos principios guían este diseño:

- **Capa declarativa por defecto** — el modder expresa intenciones, el engine las orquesta
- **Acceso raw disponible** — el modder puede bajar al nivel del `EventBus` o de los patrones siguientes si lo necesita, asumiendo la responsabilidad

### 9.1 Corrutinas — flujos que esperan decisiones

Las corrutinas son funciones que pueden pausar su ejecución (`yield`) y reanudarse más tarde desde el mismo punto, manteniendo su estado interno. Luau tiene soporte nativo.

El caso de uso principal es cualquier efecto que requiera input del jugador antes de resolverse. Sin corrutinas el engine tendría que partir el flujo en múltiples funciones y gestionar estado intermedio. Con corrutinas el modder escribe código lineal:

```lua
-- Contrahechizo: el jugador elige un target en la pila antes de resolver
function when_resolves(ctx)
    local target = coroutine.yield({
        type   = "choose_target",
        filter = function(entity)
            return entity.zone == "stack"
        end
    })

    if target == nil then return end  -- no había targets válidos, fizzle

    ctx.intent.counter_spell(target)
end
```

El engine ve el `yield`, guarda la corrutina, presenta la elección al jugador, y cuando llega la respuesta reanuda la corrutina pasando el target elegido. El modder no sabe nada de este mecanismo — su código es secuencial.

**Filtros en corrutinas — solución híbrida**

El campo `filter` de un `yield` puede ser de dos tipos, y el engine elige el camino según cuál reciba:

- **Función Luau** — máxima flexibilidad, el modder define lógica arbitraria. El engine evalúa la función contra cada entidad candidata cruzando la frontera Rust↔Luau por entidad.
- **Query ECS nativa** — eficiente, construida con los mismos mecanismos que las queries de Rust. El engine la evalúa sin cruzar la frontera.

```lua
-- Filtro como función Luau — flexible, potencialmente costoso
filter = function(entity)
    return entity.has_type("creature")
        and entity.controller ~= ctx.entity.controller
        and not entity.has_keyword("indestructible")
end

-- Filtro como query ECS nativa — eficiente, más limitado
filter = ctx.world.query()
    :with_component("Types", "Creature")
    :exclude_controller(ctx.entity.controller)
    :without_keyword("indestructible")
```

### 9.2 Chain of Responsibility — efectos de reemplazo

Permite al modder interceptar un efecto antes de que llegue a aplicarse — modificarlo o cancelarlo. Modela efectos de reemplazo, protecciones e inmunidades.

```lua
function when_enters_battlefield(ctx)
    ctx.effects.on_target(function(event)
        local es_oponente    = event.source.controller ~= ctx.entity.controller
        local apunta_jugador = event.target == ctx.entity.controller

        if es_oponente and apunta_jugador then
            return nil      -- cancelar
        end
        return event        -- dejar pasar
    end)
end
```

### 9.3 Builder — efectos compuestos legibles

En lugar de múltiples llamadas a funciones con parámetros repetidos, el modder construye un efecto completo de forma incremental. El engine recibe la intención como un objeto coherente y puede validarla y aplicar efectos de reemplazo sobre el conjunto antes de ejecutarla.

```lua
function when_resolves(ctx)
    local target = coroutine.yield({
        type   = "choose_target",
        filter = ctx.world.query():type("creature_or_player")
    })

    ctx.effect()
        :deal_damage(3)
        :to(target)
        :then_gain_life(3)
        :for_controller(ctx.entity.controller)
        :execute()
end
```

### 9.4 Middleware / Pipeline — modificar sistemas completos

El modder se inserta en el pipeline de un sistema del engine añadiendo un step que transforma el estado antes de pasarlo al siguiente. Modela efectos continuos que modifican cómo funciona un sistema mientras la carta esté en juego.

```lua
function when_enters_battlefield(ctx)
    ctx.pipeline.damage.add_step(function(state, next)
        state.amount = state.amount * 2
        return next(state)
    end)
end

function when_leaves_battlefield(ctx)
    ctx.pipeline.damage.remove_steps(ctx.entity)
end
```

### 9.5 Event Sourcing ligero — intenciones validadas

En lugar de mutar el estado del engine directamente, el modder emite *intenciones* que el engine valida y finalmente ejecuta. El modder nunca toca el estado interno del engine — siempre pasa por el sistema de validación.

```lua
function when_enters_battlefield(ctx)
    local nombre = coroutine.yield({ type = "choose_card_name" })

    ctx.intent.add_cast_restriction(function(spell)
        if spell.name == nombre then
            return false
        end
        return true
    end)
end
```

### 9.6 Composición de patrones

Los cinco patrones se pueden combinar para expresar mecánicas arbitrariamente complejas. El engine provee los ganchos, el script los combina — sin que el engine necesite conocer la mecánica de antemano.

> ⚠️ **Pendiente de refinamiento:** los nombres concretos de la API (`ctx.pipeline`, `ctx.intent`, `ctx.effects`), la granularidad de los pipelines disponibles, y cómo se integran estos patrones con el sistema de capas y la pila del juego están sin definir. Esta sección documenta la dirección, no el contrato.

---

## 10. Patrones de diseño en uso

| Patrón | Dónde | Propósito |
|---|---|---|
| **ECS** | Todas las entidades del juego | Datos composables, queries sin coste |
| **Comando / Acción** | Cambios de zona, daño, robo | Fases antes/ejecutar/después para interrupción |
| **Cola de eventos** | `EventBus` | Desacoplar productores de consumidores, evitar re-entrancy |
| **Máquina de estados** | Estructura del turno | Mantenimiento → Robo → Principal → Combate → Final |
| **Data-Driven** | Scripts Luau | Cartas como datos, no como código |
| **Observer** | `ctx.triggers` + `events` raw | Cartas escuchándose entre sí, a dos niveles de abstracción |
| **Facade** | `LuaScriptManager` | Frontera única entre engine y VM de Luau |
| **Corrutina** | Flujos con input del jugador | Código secuencial sobre flujos pausables |
| **Chain of Responsibility** | Efectos de reemplazo | Interceptación y modificación de efectos |
| **Builder** | Efectos compuestos | Construcción incremental de intenciones |
| **Middleware / Pipeline** | Modificación de sistemas | Transformaciones permanentes sobre flujos del engine |

---

## 11. Preguntas abiertas / Decisiones futuras

- [ ] Cómo gestionar `when_in_hand` y `when_on_stack` para la visibilidad de coste y color antes de entrar al campo
- [ ] Estrategia de limpieza de suscripciones para `events` raw: confirmar si el modder debe gestionar siempre el cleanup explícitamente o si el engine puede ofrecer algún mecanismo de ayuda
- [ ] Entidades token: modeladas como `Zone::Null → Zone::Battlefield` (mismo camino que cualquier ETB)
- [ ] Habilidades activadas: constructor `get_activated_abilities` vs procedure `when_activated`
- [ ] Efectos de copia: cómo duplicar componentes sin compartir `CardIdentity`
- [ ] Sistema de capas: los efectos continuos que modifican componentes necesitan un sistema de aplicación ordenado por dependencias
- [ ] Diseño concreto de `ctx.triggers` y sistemas equivalentes: qué triggers expone, cómo gestiona el orden y la elección del jugador activo
- [ ] API concreta de los patrones de control del modder (§9): nombres, granularidad de pipelines, integración con el sistema de capas y la pila del juego
- [ ] Refinamiento del sistema de filtros híbrido en corrutinas: definir cuándo el engine evalúa en Luau vs en Rust y el coste aceptable del cruce de frontera

---

## 12. Glosario

| Término | Definición |
|---|---|
| **BMoonEngine** | Nombre del motor de juego de cartas coleccionables descrito en este documento |
| **Entidad** | ID entero que representa una carta física durante toda la partida |
| **Componente** | Struct de datos puros asociado a una entidad (`PowerToughness`, `Zone`, etc.) |
| **Sistema** | Código Rust que itera todas las entidades con una combinación dada de componentes |
| **ObjectVersion** | Contador incrementado en cada cambio de zona — identifica el "nuevo objeto" según las reglas del juego |
| **CardIdentity** | Componente permanente que identifica la carta física — nunca cambia |
| **ScriptId** | `Rc<str>` que identifica un script registrado (ej. `"cards.blood_moon"`). Clave del registro y del caché del manager. String interned con refcount sin overhead atómico |
| **ScriptRegistry** | Mapa interno del manager con los metadatos de cada script registrado en el manifest |
| **LuaTableCache** | Wrapper tipado y preconfigurado de `WTinyLFUCache` que almacena las `LuaTable` cargadas, indexadas por `ScriptId`. Configurable mediante `CacheConfig` |
| **Manifest** | Archivo `manifest.toml` que describe los scripts principales de un paquete de assets. Los módulos auxiliares no se registran |
| **Procedure** | Función `when_*` en un script Luau, llamada directa y síncronamente por el engine en un cambio de zona. No se registra en el bus de eventos |
| **ctx** | DTO construido por el engine e inyectado como parámetro en cada llamada a un script. Contiene la información de contexto de la ejecución (entidad, controlador, etc.) |
| **ctx.triggers** | Ejemplo de API declarativa de alto nivel implementada por el engine y expuesta a Luau via bindings. Gestiona el orden de triggers y las reglas del juego. Uso estándar para modders |
| **events** | Tabla global en el VM de Luau con acceso directo al `EventBus` del manager. API de bajo nivel sin garantías de orden ni integración con reglas del juego. Uso avanzado |
| **EventBus** | Bus central de eventos del juego, interno al `LuaScriptManager`. Soporta los cuatro flujos: Rust→Luau, Luau→Rust, Luau→Luau, Rust→Rust. Vive en Lua app data como `Rc<RefCell<EventBus>>` |
| **HandlerId** | Token opaco devuelto por `on` en el lado Rust. Identifica un handler concreto dentro de un evento concreto y es necesario para desuscribir desde Rust via `off`. Generado por `next_handler_id()` con un `AtomicU64` estático |
| **LuaScriptManager** | Módulo autocontenido que gestiona el VM de Luau, el manifest, el registro, el `LuaTableCache`, el `EventBus` y la Scripting API Layer. Opaco al engine |
| **Scripting API Layer** | Capa de exposición estructurada en cinco fases que define cómo el engine comunica funcionalidad a los scripts Luau |
| **LuaApiModule** | Trait para módulos sin estado. Implementaciones registran funciones en una tabla global nombrada con su `namespace()` |
| **LuaContextModule** | Trait para módulos con estado compartido. El contexto se almacena como `Rc<RefCell<T>>` en Lua app data y se accede mediante `ctx_read` / `ctx_write` |
| **engine_lua** | Crate de pegamento entre `engine` y `lua_script_manager`. Implementa `LuaApiModule`/`LuaContextModule` para los módulos del engine. Único crate que depende simultáneamente de `engine`, `lua_script_manager` y `mlua` |
| **LuaTableValue** | Tipo enum del crate `lua_table` que representa los valores primitivos que puede contener una sub-tabla Luau |
| **FromLuaTable** | Trait del crate `lua_table` derivable via proc-macro. El engine lo implementa para convertir una sub-tabla Luau a un tipo propio, sin dependencia en mlua |
| **Binding** | Función Rust registrada en el VM por el engine y expuesta a Luau como parte del `ctx` (Fase 3 de la Scripting API Layer) |
| **UserData** | Struct del engine expuesto a Luau como objeto con métodos, con control explícito de visibilidad y mutabilidad (Fase 2 de la Scripting API Layer) |
| **Corrutina** | Función Luau que puede pausar su ejecución (`yield`) y reanudarse más tarde. Usada para modelar flujos que esperan input del jugador |
| **Pipeline** | Cadena de transformaciones sobre el estado de un sistema del engine. El modder puede insertar steps para modificar el comportamiento de forma permanente |