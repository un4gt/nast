//! JSONL adapter for differential tests against the checked-in ST functions.
use std::io::{self, BufRead};
use serde_json::{json, Value};
use rand::RngCore;

struct Replay(u32);
impl RngCore for Replay {
    fn next_u32(&mut self) -> u32 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 17; self.0 ^= self.0 << 5; self.0 }
    fn next_u64(&mut self) -> u64 { (self.next_u32() as u64) << 32 }
    fn fill_bytes(&mut self, bytes: &mut [u8]) { for b in bytes { *b = self.next_u32() as u8; } }
    fn try_fill_bytes(&mut self, bytes: &mut [u8]) -> Result<(), rand::Error> { self.fill_bytes(bytes); Ok(()) }
}

fn main() {
    for line in io::stdin().lock().lines() {
        let value: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let group: nast_model::group::Group = serde_json::from_value(value["group"].clone()).unwrap();
        let members: Vec<_> = value["characters"].as_array().unwrap().iter().map(|m|nast_engine::group_chat::GroupMember {
            avatar:m["avatar"].as_str().unwrap().into(), name:m["name"].as_str().unwrap().into(),
            talkativeness:m["talkativeness"].as_f64().unwrap_or(f64::NAN),
        }).collect();
        let last = if value["last"]["is_user"] == true { None } else { value["last"][if group.activation_strategy == 3 { "original_avatar" } else { "name" }].as_str() };
        let unspoken: Vec<String> = serde_json::from_value(value["unspoken"].clone()).unwrap();
        let result = nast_engine::group_chat::activate_members(&group, &members, value["input"].as_str().unwrap(), last, &unspoken, value["is_user"].as_bool().unwrap(), &mut Replay(value["seed"].as_u64().unwrap() as u32));
        println!("{}", json!(result));
    }
}
