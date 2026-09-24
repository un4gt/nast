//! Local import adapters for the formats accepted by ST's characters endpoint.
use std::{collections::BTreeMap, io::{Cursor, Read}, path::Path};
use nast_model::{Character, ChatFile, ModelError, ModelResult};
use serde_json::{json, Value};

pub struct Asset {
    pub category: &'static str,
    pub name: String,
    pub source: String,
    pub bytes: Vec<u8>,
}

pub struct ImportedCard {
    pub character: Character,
    pub avatar: Vec<u8>,
    pub assets: Vec<Asset>,
    pub chats: Vec<(String, ChatFile)>,
    pub format: &'static str,
    pub warnings: Vec<String>,
}

fn invalid(reason: impl ToString) -> ModelError {
    ModelError::InvalidValue { field: "character import", reason: reason.to_string() }
}
fn text(value: &Value) -> &str { value.as_str().unwrap_or("") }
fn array(value: &Value) -> &[Value] { value.as_array().map(Vec::as_slice).unwrap_or(&[]) }
fn image_png(bytes: &[u8]) -> ModelResult<Vec<u8>> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") { return Ok(bytes.to_vec()); }
    let image = image::load_from_memory(bytes).map_err(invalid)?;
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).map_err(invalid)?;
    Ok(out.into_inner())
}

pub fn parse(bytes: &[u8], filename: &str, user_name: &str) -> ModelResult<ImportedCard> {
    let extension = Path::new(filename).extension().and_then(|v| v.to_str()).unwrap_or("").to_lowercase();
    if extension == "charx" || extension == "byaf" || bytes.starts_with(b"PK\x03\x04") {
        let start = bytes.windows(4).position(|w| w == b"PK\x03\x04").ok_or_else(|| invalid("missing ZIP header"))?;
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes[start..])).map_err(invalid)?;
        let mut files = BTreeMap::new();
        let mut total = 0_u64;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(invalid)?;
            if file.is_dir() { continue; }
            let name = file.enclosed_name().ok_or_else(|| invalid("invalid archive path"))?
                .to_string_lossy().replace('\\', "/");
            total = total.checked_add(file.size()).ok_or_else(|| invalid("archive too large"))?;
            if total > 256 * 1024 * 1024 { return Err(invalid("expanded archive exceeds 256 MiB")); }
            let mut data = Vec::new();
            file.by_ref().take(256 * 1024 * 1024 + 1).read_to_end(&mut data).map_err(invalid)?;
            if data.len() > 256 * 1024 * 1024 { return Err(invalid("archive entry too large")); }
            if files.insert(name, data).is_some() { return Err(invalid("duplicate archive path")); }
        }
        return if extension == "byaf" || !files.contains_key("card.json") {
            byaf(&files, user_name)
        } else { charx(&files) };
    }
    let (mut value, avatar, format) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        (crate::extract_card_json(bytes)?.0, bytes.to_vec(), "png")
    } else if matches!(extension.as_str(), "yaml" | "yml") {
        let yaml: Value = serde_yaml::from_slice(bytes).map_err(invalid)?;
        (json!({"name": yaml["name"], "description": text(&yaml["context"]),
            "first_mes": text(&yaml["greeting"])}), crate::minimal_png(), "yaml")
    } else {
        (serde_json::from_slice::<Value>(bytes)?, crate::minimal_png(), "json")
    };
    if value.get("name").is_none() && value.get("char_name").is_some() {
        value = json!({"name": value["char_name"], "description": text(&value["char_persona"]),
            "first_mes": text(&value["char_greeting"]), "scenario": text(&value["world_scenario"]),
            "mes_example": text(&value["example_dialogue"])});
    }
    let character = Character::from_card_json(&value)?;
    character.validate()?;
    Ok(ImportedCard { character, avatar, assets: vec![], chats: vec![], format, warnings: vec![] })
}

fn read_json(files: &BTreeMap<String, Vec<u8>>, name: &str) -> ModelResult<Value> {
    Ok(serde_json::from_slice(files.get(name).ok_or_else(|| invalid(format!("missing {name}")))?)?)
}
fn extension(name: &str) -> String {
    Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("png").to_lowercase()
}
fn normalized_name(name: &str, fallback: &str, separator: char) -> String {
    let mut output = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() { output.push(c); }
        else if !output.ends_with(separator) { output.push(separator); }
    }
    let output = output.trim_matches(separator);
    if output.is_empty() { fallback.into() } else { output.into() }
}

fn charx(files: &BTreeMap<String, Vec<u8>>) -> ModelResult<ImportedCard> {
    let value = read_json(files, "card.json")?;
    if value.get("spec").is_none() { return Err(invalid("CHARX card is missing spec")); }
    let character = Character::from_card_json(&value)?;
    character.validate()?;
    let mut result = ImportedCard { character, avatar: crate::minimal_png(),
        assets: vec![], chats: vec![], format: "charx", warnings: vec![] };
    let mut icons = Vec::new();
    for (index, asset) in array(&value["data"]["assets"]).iter().enumerate() {
        let uri = text(&asset["uri"]).trim();
        let Some(prefix) = ["embeded://", "embedded://", "__asset:"].into_iter()
            .find(|prefix| uri.to_lowercase().starts_with(prefix)) else { continue; };
        let path = uri[prefix.len()..].replace('\\', "/");
        let path = path.trim_start_matches("./");
        let ext = if text(&asset["ext"]).is_empty() { extension(path) }
            else { text(&asset["ext"]).trim_start_matches('.').to_lowercase() };
        if !["png","jpg","jpeg","webp","gif","apng","avif","bmp","jfif"].contains(&ext.as_str()) { continue; }
        let Some(bytes) = files.get(path) else {
            result.warnings.push(format!("Missing asset: {path}")); continue;
        };
        let kind = text(&asset["type"]).to_lowercase();
        if kind == "icon" { icons.push((text(&asset["name"]).eq_ignore_ascii_case("main"), bytes)); continue; }
        if kind == "user_icon" { continue; }
        let category = match kind.as_str() { "emotion" | "expression" => "sprite", "background" => "background", _ => "misc" };
        let name = text(&asset["name"]);
        let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or(name);
        let base = normalized_name(stem, &format!("{category}-{index}"), if category == "sprite" { '-' } else { '_' });
        result.assets.push(Asset { category, name: format!("{base}.{ext}"), source: uri.into(), bytes: bytes.clone() });
    }
    if let Some((_, bytes)) = icons.iter().find(|(main, _)| *main).or(icons.first()) {
        result.avatar = image_png(bytes)?;
    }
    Ok(result)
}

fn macros(value: &Value) -> String {
    let text = text(value);
    let text = regex_replace(text, r"(?i)#\{user\}:", "{{user}}:");
    let text = regex_replace(&text, r"(?i)#\{character\}:", "{{char}}:");
    // Match the unbraced prefix as well, so existing double-braced macros stay intact.
    let text = regex_replace(&text, r"(?i)(^|[^\{])\{character\}", "${1}{{char}}");
    regex_replace(&text, r"(?i)(^|[^\{])\{user\}", "${1}{{user}}")
}
fn regex_replace(text: &str, pattern: &str, replacement: &str) -> String {
    regex::Regex::new(pattern).unwrap().replace_all(text, replacement).into_owned()
}
fn examples(scenario: &Value) -> String {
    array(&scenario["exampleMessages"]).iter().filter(|e| !text(&e["text"]).is_empty())
        .map(|e| format!("<START>\n{}\n", macros(&e["text"]))).collect::<String>().trim_end().into()
}

fn byaf(files: &BTreeMap<String, Vec<u8>>, user_name: &str) -> ModelResult<ImportedCard> {
    let manifest = read_json(files, "manifest.json")?;
    let character_path = manifest["characters"][0].as_str().ok_or_else(|| invalid("BYAF missing character path"))?;
    let source = read_json(files, character_path)?;
    let mut scenarios = Vec::new();
    let mut warnings = Vec::new();
    if array(&manifest["characters"]).len() > 1 { warnings.push("BYAF: only the first character is imported, matching ST".into()); }
    for path in array(&manifest["scenarios"]) {
        match read_json(files, text(path)) { Ok(v) => scenarios.push(v), Err(e) => warnings.push(e.to_string()) }
    }
    if scenarios.is_empty() { scenarios.push(json!({})); }
    let first = &scenarios[0];
    let name = source["name"].as_str().filter(|s| !s.is_empty()).unwrap_or(text(&source["displayName"]));
    let mut greetings = Vec::new();
    for scenario in scenarios.iter().skip(1) {
        if !text(&scenario["firstMessages"][0]["text"]).is_empty()
            && scenario["firstMessages"][0]["text"] != first["firstMessages"][0]["text"] {
            let greeting = macros(&scenario["firstMessages"][0]["text"]);
            if !greetings.contains(&greeting) { greetings.push(greeting); }
        }
    }
    let lore: Vec<Value> = array(&source["loreItems"]).iter().enumerate().filter(|(_, e)| !e.is_null())
        .map(|(index, entry)| json!({"keys": macros(&entry["key"]).split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>(),
            "content": macros(&entry["value"]), "extensions": {}, "enabled": true, "insertion_order": index})).collect();
    let value = json!({"spec":"chara_card_v2", "spec_version":"2.0", "data":{
        "name":name, "description":macros(&source["persona"]), "scenario":macros(&first["narrative"]),
        "first_mes":macros(&first["firstMessages"][0]["text"]), "mes_example":examples(first),
        "creator_notes":text(&manifest["author"]["backyardURL"]), "creator":text(&manifest["author"]["name"]),
        "system_prompt":macros(&first["formattingInstructions"]), "alternate_greetings":greetings,
        "tags":if source["isNSFW"] == true {vec!["nsfw"]} else {vec![]},
        "character_book":if lore.is_empty() {Value::Null} else {json!({"entries":lore,"extensions":{}})},
        "extensions":{"display_name":source["displayName"]}}});
    let character = Character::from_card_json(&value)?;
    character.validate()?;
    let mut result = ImportedCard { character, avatar:crate::minimal_png(), assets:vec![], chats:vec![], format:"byaf", warnings };
    let mut found_avatar = false;
    for icon in array(&source["images"]) {
        let relative = Path::new(character_path).parent().unwrap_or(Path::new("")).join(text(&icon["path"]));
        let Some(bytes) = files.get(&relative.to_string_lossy().replace('\\', "/")) else {continue;};
        if !found_avatar { result.avatar = image_png(bytes)?; found_avatar=true; }
        else {
            let label = crate::sanitize_filename(text(&icon["label"]));
            result.assets.push(Asset {category:"sprite", name:format!("{label}.{}",extension(text(&icon["path"]))),
                source:text(&icon["path"]).into(), bytes:bytes.clone()});
        }
    }
    for (index, scenario) in scenarios.iter().enumerate() {
        if let Some(bytes) = files.get(text(&scenario["backgroundImage"])) {
            result.assets.push(Asset { category:"byaf_background", name:format!("bg_{index}.{}", extension(text(&scenario["backgroundImage"]))),
                source:text(&scenario["backgroundImage"]).into(), bytes:bytes.clone() });
        }
        let chat = byaf_chat(scenario, user_name, name);
        let title = scenario["title"].as_str().filter(|s| !s.is_empty()).unwrap_or(name);
        result.chats.push((format!("{} {index}", crate::sanitize_filename(title)), chat));
    }
    Ok(result)
}

fn byaf_chat(scenario: &Value, user: &str, character: &str) -> ChatFile {
    let mut metadata = json!({"scenario":text(&scenario["narrative"]), "mes_example":examples(scenario),
        "system_prompt":macros(&scenario["formattingInstructions"]),
        "mes_examples_optional":scenario.get("canDeleteExampleMessages").cloned().unwrap_or(json!(false)),
        "chat_backgrounds":[], "custom_background":"", "byaf_model_settings":{}});
    for (target, source, default) in [
        ("model","model",json!("")), ("temperature","temperature",json!(1.2)), ("top_k","topK",json!(40)),
        ("top_p","topP",json!(0.9)), ("min_p","minP",json!(0.1)), ("min_p_enabled","minPEnabled",json!(true)),
        ("repeat_penalty","repeatPenalty",json!(1.05)), ("repeat_penalty_tokens","repeatLastN",json!(256)),
        ("by_prompt_template","promptTemplate",json!("general")), ("grammar","grammar",Value::Null),
    ] { metadata["byaf_model_settings"][target] = scenario.get(source).filter(|v| !v.is_null()).cloned().unwrap_or(default); }
    // The persister resolves the background to the final unique avatar directory.
    metadata["nast_import_background"] = scenario["backgroundImage"].clone();
    let mut chat = vec![json!({"user_name":"unused","character_name":"unused","chat_metadata":metadata})];
    if !text(&scenario["firstMessages"][0]["text"]).is_empty() {
        chat.push(json!({"name":character,"is_user":false,"send_date":scenario["messages"][0]["createdAt"],
            "mes":scenario["firstMessages"][0]["text"]}));
    }
    let messages = array(&scenario["messages"]);
    let human:Vec<_> = messages.iter().filter(|m| m["type"] == "human").collect();
    let ai:Vec<_> = messages.iter().filter(|m| m["type"] == "ai").collect();
    let ordered:Vec<_> = if human.len() == ai.len() { human.into_iter().zip(ai).flat_map(|(u,a)| [u,a]).collect() }
        else { messages.iter().collect() };
    for message in ordered {
        if message["type"] == "human" {
            chat.push(json!({"name":user,"is_user":true,"send_date":message["createdAt"],"mes":message["text"]}));
        } else {
            let outputs = array(&message["outputs"]);
            let Some((index, active)) = outputs.iter().enumerate().max_by_key(|(_,o)| {
                o["activeTimestamp"].as_i64()
                    .or_else(|| chrono::DateTime::parse_from_rfc3339(text(&o["activeTimestamp"])).ok().map(|date| date.timestamp_millis()))
                    .or_else(|| text(&o["activeTimestamp"]).parse().ok()).unwrap_or(0)
            }) else {continue;};
            chat.push(json!({"name":character,"is_user":false,"send_date":active["createdAt"],"mes":active["text"],
                "swipes":outputs.iter().map(|o| &o["text"]).collect::<Vec<_>>(),"swipe_id":index}));
        }
    }
    ChatFile(chat)
}
