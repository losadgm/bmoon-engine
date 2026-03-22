use engine::test_state::TestState;
use engine_lua::test_state::TestStateLua;
use lua_script_manager::{CallResult, LuaScriptManager};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Crea un directorio temporal con un script Luau y su manifest, inicializa
/// el manager y bindea `TestState`. El `TempDir` debe mantenerse vivo durante todo el test.
fn setup(script_id: &str, script_source: &str) -> (tempfile::TempDir, LuaScriptManager) {
    let dir = tempfile::tempdir().expect("tempdir");

    let scripts_dir = dir.path().join("scripts");
    std::fs::create_dir_all(&scripts_dir).unwrap();
    std::fs::write(scripts_dir.join("script.luau"), script_source).unwrap();

    std::fs::write(
        dir.path().join("manifest.toml"),
        format!(
            "[package]\nname = \"test_pkg\"\n\n[[script]]\nid = \"{script_id}\"\npath = \"scripts/script.luau\"\n"
        ),
    )
    .unwrap();

    let manager = LuaScriptManager::new(100, &[dir.path()]).expect("manager init");
    manager
        .bind_module::<TestStateLua>(TestState::default())
        .expect("bind TestApi");

    (dir, manager)
}

// ── get_table ─────────────────────────────────────────────────────────────────

#[test]
fn get_table_converts_define_block() {
    use lua_table::FromLuaTable;

    #[derive(FromLuaTable, Debug)]
    struct CardDef {
        cost: String,
        power: Option<i64>,
        keywords: Vec<String>,
    }

    let (_dir, mut manager) = setup(
        "cards.test",
        r#"
define = {
    cost     = "2G",
    power    = 3,
    keywords = { "trample", "haste" },
}
"#,
    );

    let def: CardDef = manager
        .get_table("cards.test", "define")
        .expect("get_table should return Some");

    assert_eq!(def.cost, "2G");
    assert_eq!(def.power, Some(3));
    assert_eq!(def.keywords, vec!["trample", "haste"]);
}

#[test]
fn get_table_returns_none_for_missing_key() {
    #[derive(lua_table::FromLuaTable)]
    struct Dummy {
        _unused: String,
    }

    let (_dir, mut manager) = setup("cards.empty", "-- script vacío");
    let result: Option<Dummy> = manager.get_table("cards.empty", "define");
    assert!(result.is_none());
}

// ── call ──────────────────────────────────────────────────────────────────────

#[test]
fn call_returns_function_not_found_when_procedure_absent() {
    let (_dir, mut manager) = setup("cards.no_etb", r#"define = { cost = "G" }"#);

    let result = manager.call::<_, ()>("cards.no_etb", "when_enters_battlefield", ());

    assert!(
        matches!(result, CallResult::FunctionNotFound),
        "expected FunctionNotFound, got {result:?}"
    );
}

#[test]
fn call_executes_procedure_and_mutates_state() {
    let (_dir, mut manager) = setup(
        "cards.etb",
        r#"
function when_enters_battlefield()
    test.increment()
    test.increment()
end
"#,
    );

    let result = manager.call::<_, ()>("cards.etb", "when_enters_battlefield", ());
    assert!(matches!(result, CallResult::Ok(())));

    manager.with_state::<TestStateLua, _, _>(|state| {
        assert_eq!(state.counter, 2);
    });
}

#[test]
fn call_returns_script_error_on_runtime_error() {
    let (_dir, mut manager) = setup(
        "cards.broken",
        r#"
function when_resolves()
    error("algo salió mal")
end
"#,
    );

    let result = manager.call::<_, ()>("cards.broken", "when_resolves", ());
    assert!(
        matches!(result, CallResult::ScriptError(_)),
        "expected ScriptError, got {result:?}"
    );
}

// ── events ────────────────────────────────────────────────────────────────────

#[test]
fn events_on_and_emit_rust_to_lua() {
    let (_dir, mut manager) = setup(
        "cards.events_test",
        r#"
function when_enters_battlefield()
    events.on("creature_died", function(name)
        test.record("died", name)
    end)
end
"#,
    );

    manager.call::<_, ()>("cards.events_test", "when_enters_battlefield", ());
    manager.emit("creature_died", "Llanowar Elves").unwrap();

    manager.with_state::<TestStateLua, _, _>(|state| {
        assert_eq!(
            state.calls.get("died").unwrap(),
            &vec!["Llanowar Elves".to_string()]
        );
    });
}

#[test]
fn events_off_by_function_reference_removes_handler() {
    let (_dir, mut manager) = setup(
        "cards.off_test",
        r#"
local handler = function()
    test.increment()
end

function when_enters_battlefield()
    events.on("tick", handler)
end

function when_leaves_battlefield()
    events.off("tick", handler)
end
"#,
    );

    manager.call::<_, ()>("cards.off_test", "when_enters_battlefield", ());
    manager.emit("tick", ()).unwrap(); // counter → 1

    manager.call::<_, ()>("cards.off_test", "when_leaves_battlefield", ());
    manager.emit("tick", ()).unwrap(); // desuscrito → sigue en 1

    manager.with_state::<TestStateLua, _, _>(|state| {
        assert_eq!(state.counter, 1);
    });
}

#[test]
fn events_lua_to_lua() {
    let (_dir, mut manager) = setup(
        "cards.lua_to_lua",
        r#"
function when_enters_battlefield()
    events.on("ping", function()
        test.increment()
    end)
end

function when_resolves()
    events.emit("ping")
end
"#,
    );

    manager.call::<_, ()>("cards.lua_to_lua", "when_enters_battlefield", ());
    manager.call::<_, ()>("cards.lua_to_lua", "when_resolves", ());
    manager.call::<_, ()>("cards.lua_to_lua", "when_resolves", ());

    manager.with_state::<TestStateLua, _, _>(|state| {
        assert_eq!(state.counter, 2);
    });
}

#[test]
fn rust_handler_off_by_handler_id() {
    use std::cell::Cell;
    use std::rc::Rc;

    let fired = Rc::new(Cell::new(0u32));
    let fired_clone = fired.clone();

    let (_dir, manager) = setup("cards.rust_handler", "-- script vacío");

    let id = manager
        .on("zone_changed", move |_lua, _args| {
            fired_clone.set(fired_clone.get() + 1);
            Ok(())
        })
        .unwrap();

    manager.emit("zone_changed", ()).unwrap(); // fired → 1
    manager.off("zone_changed", id).unwrap();
    manager.emit("zone_changed", ()).unwrap(); // desuscrito → sigue en 1

    assert_eq!(fired.get(), 1);
}
