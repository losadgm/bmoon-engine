use std::collections::HashMap;

use lua_table::{FromLuaTable, LuaTableValue};

// ─── Helper ──────────────────────────────────────────────────────────────────

fn map(pairs: &[(&str, LuaTableValue)]) -> HashMap<String, LuaTableValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

// ─── Structs under test ──────────────────────────────────────────────────────

#[derive(FromLuaTable, Debug, PartialEq)]
pub struct CardDefinition {
    cost: String,
    power: Option<i64>,
    toughness: Option<i64>,
    keywords: Vec<String>,
}

#[derive(FromLuaTable, Debug, PartialEq)]
pub struct AllPrimitives {
    s: String,
    i: i64,
    f: f64,
    b: bool,
}

#[derive(FromLuaTable, Debug, PartialEq)]
pub struct WithNested {
    name: String,
    stats: AllPrimitives,
}

#[derive(FromLuaTable, Debug, PartialEq)]
pub struct WithOptionalNested {
    name: String,
    extra: Option<AllPrimitives>,
}

#[derive(FromLuaTable, Debug, PartialEq)]
pub struct WithVecNested {
    label: String,
    items: Vec<AllPrimitives>,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn card_definition_full() {
    let m = map(&[
        ("cost", LuaTableValue::String("G".into())),
        ("power", LuaTableValue::Int(1)),
        ("toughness", LuaTableValue::Int(1)),
        (
            "keywords",
            LuaTableValue::List(vec![
                LuaTableValue::String("vigilance".into()),
                LuaTableValue::String("flying".into()),
            ]),
        ),
    ]);
    let card = CardDefinition::from_lua_table(m).unwrap();
    assert_eq!(card.cost, "G");
    assert_eq!(card.power, Some(1));
    assert_eq!(card.toughness, Some(1));
    assert_eq!(card.keywords, vec!["vigilance", "flying"]);
}

#[test]
fn card_definition_optional_fields_absent() {
    let m = map(&[
        ("cost", LuaTableValue::String("1G".into())),
        ("keywords", LuaTableValue::List(vec![])),
    ]);
    let card = CardDefinition::from_lua_table(m).unwrap();
    assert_eq!(card.power, None);
    assert_eq!(card.toughness, None);
    assert_eq!(card.keywords, Vec::<String>::new());
}

#[test]
fn missing_required_field_errors() {
    // 'cost' is required but absent
    let m = map(&[("keywords", LuaTableValue::List(vec![]))]);
    let err = CardDefinition::from_lua_table(m).unwrap_err();
    assert!(
        err.contains("cost"),
        "error should mention the missing field: {err}"
    );
}

#[test]
fn wrong_variant_for_required_field_errors() {
    let m = map(&[
        ("cost", LuaTableValue::Int(42)), // should be String
        ("keywords", LuaTableValue::List(vec![])),
    ]);
    let err = CardDefinition::from_lua_table(m).unwrap_err();
    assert!(
        err.contains("cost"),
        "error should mention the offending field: {err}"
    );
}

#[test]
fn wrong_variant_for_optional_field_errors() {
    let m = map(&[
        ("cost", LuaTableValue::String("G".into())),
        ("power", LuaTableValue::String("nope".into())), // should be Int
        ("keywords", LuaTableValue::List(vec![])),
    ]);
    let err = CardDefinition::from_lua_table(m).unwrap_err();
    assert!(
        err.contains("power"),
        "error should mention the offending field: {err}"
    );
}

#[test]
fn all_primitives_round_trip() {
    let m = map(&[
        ("s", LuaTableValue::String("hello".into())),
        ("i", LuaTableValue::Int(-7)),
        ("f", LuaTableValue::Float(3.14)),
        ("b", LuaTableValue::Bool(true)),
    ]);
    let v = AllPrimitives::from_lua_table(m).unwrap();
    assert_eq!(v.s, "hello");
    assert_eq!(v.i, -7);
    assert!((v.f - 3.14).abs() < 1e-9);
    assert!(v.b);
}

#[test]
fn nested_struct_required() {
    let stats_map = map(&[
        ("s", LuaTableValue::String("x".into())),
        ("i", LuaTableValue::Int(0)),
        ("f", LuaTableValue::Float(0.0)),
        ("b", LuaTableValue::Bool(false)),
    ]);
    let m = map(&[
        ("name", LuaTableValue::String("Card".into())),
        ("stats", LuaTableValue::Map(stats_map)),
    ]);
    let v = WithNested::from_lua_table(m).unwrap();
    assert_eq!(v.name, "Card");
    assert_eq!(v.stats.i, 0);
}

#[test]
fn nested_struct_optional_present() {
    let extra_map = map(&[
        ("s", LuaTableValue::String("y".into())),
        ("i", LuaTableValue::Int(99)),
        ("f", LuaTableValue::Float(1.0)),
        ("b", LuaTableValue::Bool(true)),
    ]);
    let m = map(&[
        ("name", LuaTableValue::String("Thing".into())),
        ("extra", LuaTableValue::Map(extra_map)),
    ]);
    let v = WithOptionalNested::from_lua_table(m).unwrap();
    assert_eq!(v.extra.unwrap().i, 99);
}

#[test]
fn nested_struct_optional_absent() {
    let m = map(&[("name", LuaTableValue::String("Thing".into()))]);
    let v = WithOptionalNested::from_lua_table(m).unwrap();
    assert!(v.extra.is_none());
}

#[test]
fn vec_of_nested_structs() {
    let make_item = |i: i64| {
        LuaTableValue::Map(map(&[
            ("s", LuaTableValue::String("".into())),
            ("i", LuaTableValue::Int(i)),
            ("f", LuaTableValue::Float(0.0)),
            ("b", LuaTableValue::Bool(false)),
        ]))
    };
    let m = map(&[
        ("label", LuaTableValue::String("list".into())),
        (
            "items",
            LuaTableValue::List(vec![make_item(1), make_item(2), make_item(3)]),
        ),
    ]);
    let v = WithVecNested::from_lua_table(m).unwrap();
    let ints: Vec<i64> = v.items.iter().map(|x| x.i).collect();
    assert_eq!(ints, vec![1, 2, 3]);
}

#[test]
fn vec_wrong_element_variant_errors() {
    let m = map(&[
        ("cost", LuaTableValue::String("G".into())),
        (
            "keywords",
            LuaTableValue::List(vec![
                LuaTableValue::Int(42), // should be String
            ]),
        ),
    ]);
    let err = CardDefinition::from_lua_table(m).unwrap_err();
    assert!(
        err.contains("keywords"),
        "error should mention the offending field: {err}"
    );
}
