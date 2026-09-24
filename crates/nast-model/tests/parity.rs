use nast_model::{Character, WorldInfoBook, WIEntry, RegexScript};
use serde_json::json;

#[test]
fn st_v3_card_and_embedded_book_roundtrip() {
    let card = Character::from_card_json(&json!({
        "spec": "chara_card_v3", "spec_version": "3.0", "vendorRoot": {"value": 42},
        "data": {"name": "测试角色", "extensions": {"talkativeness": 0.7},
        "character_book": {"name": "角色书", "vendorBook": true, "entries": [{
            "id": 7, "keys": ["城堡"], "content": "旧内容", "enabled": true,
            "vendorEntry": "keep", "extensions": {"delay_until_recursion": 2,
                "sticky": null, "cooldown": null, "delay": null, "selectiveLogic": 3,
                "useProbability": false, "position": 7, "outlet_name": "secret", "vendor": 42}
        }]}}
    })).unwrap();
    assert_eq!(card.talkativeness.as_deref(), Some("0.7"));
    let mut world = WorldInfoBook::from_embedded(card.data.character_book.as_ref().unwrap()).unwrap();
    let entry = world.entries.get_mut("7").unwrap();
    assert_eq!(entry.delay_until_recursion.level(), 2);
    assert_eq!(entry.selective_logic, 3);
    assert!(!entry.use_probability);
    assert_eq!(entry.outlet_name.as_deref(), Some("secret"));
    entry.content = "新内容".into();
    let rebuilt = serde_json::to_value(world.to_embedded("角色书").unwrap()).unwrap();
    assert_eq!(rebuilt["entries"][0]["content"], "新内容");
    assert_eq!(rebuilt["entries"][0]["vendorEntry"], "keep");
    assert_eq!(rebuilt["entries"][0]["extensions"]["vendor"], 42);
    assert_eq!(rebuilt["vendorBook"], true);
    let serialized = serde_json::to_value(card).unwrap();
    assert_eq!(serialized["spec"], "chara_card_v3");
    assert_eq!(serialized["spec_version"], "3.0");
    assert_eq!(serialized["vendorRoot"]["value"], 42);
}

#[test]
fn st_world_fields_are_consumed_and_legacy_names_still_load() {
    for wire in [json!({"scanDepth": 2, "ignoreBudget": true, "preventRecursion": true}),
                 json!({"scan_depth": 2, "ignore_budget": true, "prevent_recursion": true})] {
        let entry: WIEntry = serde_json::from_value(wire).unwrap();
        assert_eq!(entry.scan_depth, Some(2));
        assert!(entry.ignore_budget && entry.prevent_recursion);
        assert!(!entry.extra.contains_key("ignoreBudget"));
        let output = serde_json::to_value(entry).unwrap();
        assert_eq!(output["scanDepth"], 2);
        assert!(output.get("scan_depth").is_none());
    }
    let entry: WIEntry = serde_json::from_value(json!({"sticky":null,"cooldown":null,"delay":null})).unwrap();
    assert_eq!((entry.sticky, entry.cooldown, entry.delay), (0, 0, 0));
}

#[test]
fn st_regex_wire_names_are_not_silently_dropped() {
    let script: RegexScript = serde_json::from_value(json!({
        "scriptName": "测试", "findRegex": "/foo/gi", "replaceString": "bar",
        "promptOnly": true, "minDepth": 2
    })).unwrap();
    assert_eq!(script.find_regex, "/foo/gi");
    assert!(script.prompt_only);
    assert_eq!(script.min_depth, Some(2));
}

#[test]
fn missing_embedded_extensions_use_st_defaults() {
    let entry: nast_model::card::CharacterBookEntry = serde_json::from_value(json!({
        "keys":["castle"], "content":"lore", "enabled":true
    })).unwrap();
    assert_eq!(entry.extensions.probability, 100);
    assert!(entry.extensions.use_probability);
    assert_eq!(entry.extensions.depth, 4);
    assert_eq!(entry.extensions.group_weight, 100);
}

#[test]
fn embedded_book_accepts_mixed_legacy_extension_names() {
    for extensions in [
        json!({"selective_logic":0,"selectiveLogic":3,"use_probability":true,"useProbability":false,"vendor":"keep"}),
        json!({"useProbability":false,"use_probability":true,"selectiveLogic":3,"selective_logic":0,"vendor":"keep"}),
        json!({"selective_logic":3,"use_probability":false,"vendor":"keep"}),
    ] {
        let card = Character::from_card_json(&json!({
            "spec":"chara_card_v2", "spec_version":"2.0", "data":{
                "name":"Legacy card", "character_book":{"entries":[{
                    "keys":["castle"], "enabled":true, "extensions":extensions
                }]}
            }
        })).unwrap();
        let book = card.data.character_book.as_ref().unwrap();
        let ext = &book.entries[0].extensions;
        assert_eq!(ext.selective_logic, 3);
        assert!(!ext.use_probability);
        assert_eq!(ext.extra["vendor"], "keep");
        let world = WorldInfoBook::from_embedded(book).unwrap();
        assert_eq!(world.entries["0"].selective_logic, 3);
        assert!(!world.entries["0"].use_probability);
        let output = serde_json::to_value(ext).unwrap();
        assert_eq!(output["selectiveLogic"], 3);
        assert_eq!(output["useProbability"], false);
        assert!(output.get("selective_logic").is_none());
        assert!(output.get("use_probability").is_none());
        Character::from_card_json(&serde_json::to_value(&card).unwrap()).unwrap();
    }
}

#[test]
fn invalid_canonical_embedded_extension_is_not_hidden_by_legacy_value() {
    let result = serde_json::from_value::<nast_model::card::CharacterBookEntry>(json!({
        "keys":[], "extensions":{"selectiveLogic":"invalid", "selective_logic":0}
    }));
    assert!(result.is_err());
}
