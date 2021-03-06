// Copyright (c) 2016 Nikita Pekin and the smexybot contributors
// See the README.md file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Provides functionality for the `tag` command.

extern crate uuid;

use chrono::{DateTime, UTC};
use self::uuid::Uuid;
use serde_json;
use serenity::client::{Context, rest};
use serenity::model::{GuildId, Message, UserId};
use serenity::utils::builder::CreateEmbed;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::sync::Mutex;
use util::{check_msg, merge, timestamp_to_string};

lazy_static! {
    static ref TAGS: Tags = Tags {
        config: Mutex::new(Config::new("tags.json")),
    };
}

#[cfg(feature = "nightly")]
include!("tag.in.rs");

#[cfg(feature = "with-syntex")]
include!(concat!(env!("OUT_DIR"), "/tag.rs"));

impl Tag {
    fn new(
        name: String,
        content: String,
        owner_id: u64,
        uses: Option<u32>,
        location: Option<String>,
        created_at: Option<DateTime<UTC>>
    ) -> Self {
        Tag {
            name: name,
            content: content,
            owner_id: owner_id,
            uses: uses.unwrap_or(0),
            location: location,
            created_at: created_at.unwrap_or_else(UTC::now),
        }
    }

    fn as_embed(&self, embed: CreateEmbed) -> CreateEmbed {
        embed.title(&self.name)
            .field(|f| f.name("Owner").value(&format!("<@!{}>", self.owner_id)))
            .field(|f| f.name("Uses").value(&self.uses.to_string()))
            .author(|a| {
                let owner_id = UserId(self.owner_id);
                let (name, avatar_url) = match owner_id.find() {
                    Some(user) => (user.name.clone(), user.avatar_url()),
                    None => {
                        match rest::get_user(owner_id.0) {
                            Ok(user) => (user.name.clone(), user.avatar_url()),
                            Err(_) => return a,
                        }
                    },
                };
                let mut a = a.name(&name);
                if let Some(avatar_url) = avatar_url {
                    a = a.icon_url(&avatar_url);
                }
                a
            })
            .timestamp(timestamp_to_string(&self.created_at))
            .footer(|f| {
                f.text(if self.is_generic() {
                    "Generic"
                } else {
                    "Server-specific"
                })
            })
    }

    fn is_generic(&self) -> bool {
        self.location.is_none()
    }
}

#[derive(Debug)]
struct Config {
    name: String,
    tags: HashMap<String, HashMap<String, Tag>>,
}

impl Config {
    fn new(name: &str) -> Self {
        let mut config = Config {
            name: name.to_owned(),
            tags: HashMap::new(),
        };

        config.load();

        config
    }

    fn get(&self, key: &str) -> Option<&HashMap<String, Tag>> {
        self.tags.get(key)
    }

    fn insert(&mut self, key: String, value: HashMap<String, Tag>) {
        self.tags.insert(key, value);
        self.save();
    }

    fn load(&mut self) {
        let mut file = match File::open(&self.name) {
            Ok(file) => file,
            // If no file is present, assume this is a fresh config.
            Err(ref err) if err.kind() == ErrorKind::NotFound => return,
            Err(_) => panic!("Failed to open file: {}", self.name),
        };
        let mut tags = String::new();
        file.read_to_string(&mut tags)
            .expect(&format!("Failed to read from file: {}", self.name));
        self.tags = serde_json::from_str(&tags).expect("Failed to deserialize Config");
        debug!("Loaded config from: {}", self.name);
    }

    fn save(&self) {
        let temp = format!("{}-{}.tmp", Uuid::new_v4(), self.name);
        let mut file = File::create(&temp).expect(&format!("Failed to create file: {}", temp));
        file.write_all(serde_json::to_string(&self.tags)
                .expect("Failed to serialize Config")
                .as_bytes())
            .expect(&format!("Failed to write to file: {}", temp));

        // Atomically copy the new config.
        fs::rename(temp, &self.name).expect("Failed to write new Config");
        trace!("Saved config to: {}", self.name);
    }
}

#[derive(Debug)]
struct Tags {
    config: Mutex<Config>,
}

impl Tags {
    fn get_possible_tags(&self, guild: Option<GuildId>) -> HashMap<String, Tag> {
        let config = self.config.lock().expect("Failed to lock Config");
        let generic = config.get("generic")
            .cloned()
            .unwrap_or_else(HashMap::new);

        match guild {
            None => generic,
            Some(guild) => {
                merge(generic,
                      config.get(&guild.to_string())
                          .cloned()
                          .unwrap_or_else(HashMap::new))
            },
        }
    }

    fn get_tag(&self, guild: Option<GuildId>, name: String) -> Result<Tag, String> {
        self.get_possible_tags(guild)
            .get(&name)
            .cloned()
            .ok_or_else(|| "Tag not found".to_owned())
    }

    fn put_tag(&self, guild: Option<GuildId>, name: String, tag: Tag) {
        // Load the actual tag so we can modify it.
        let mut config = TAGS.config
            .lock()
            .expect("Failed to lock Config");
        {
            let database = config.tags
                .get_mut(&get_database_location(guild))
                .unwrap();
            database.insert(name, tag);
        }
        config.save();
    }

    fn delete_tag(&self, guild: Option<GuildId>, name: &str) {
        let mut config = TAGS.config
            .lock()
            .expect("Failed to lock Config");
        {
            let database = config.tags
                .get_mut(&get_database_location(guild))
                .unwrap();
            database.remove(name);
        }
        config.save();
    }
}

command!(tag(context, message, args) {
    let mut args = args.into_iter();

    let f = match args.next().as_ref().map(String::as_ref) {
        Some("create") => create,
        Some("info") => info,
        Some("list") => list,
        Some("edit") => edit,
        Some("delete") => delete,
        Some(name) => {
            return {
                let guild_id = message.guild_id();

                let lookup = name.to_lowercase();
                match TAGS.get_tag(guild_id, lookup.clone()) {
                    Ok(tag) => {
                        let mut tag = tag.clone();
                        tag.uses += 1;
                        TAGS.put_tag(guild_id, lookup, tag.clone());
                        check_msg(context.say(&tag.content));

                        Ok(())
                    },
                    Err(err) => Err(err),
                }
            };
        },
        None => {
            return Err("Either specify a tag name or use one of the available commands."
                .to_owned());
        },
    };

    // This is necessary because the `command!` macro returns `Ok(())`. Without
    // this match and fall-through, rustc would complain about unreachable code.
    match f(context, message, args.collect()) {
        Ok(()) => {},
        v => return v,
    }
});

pub fn create(context: &Context, message: &Message, args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();

    let name = match args.next() {
        Some(name) => name,
        None => return Err("Please specify a name for the tag.".to_owned()),
    };

    let content = args.collect::<Vec<String>>();
    let content = if content.is_empty() {
        return Err("Please specify some content for the tag.".to_owned());
    } else {
        content.join(" ")
    };

    let name = name.trim().to_lowercase().to_owned();
    verify_tag_name(&name)?;

    let location = get_database_location(message.guild_id());
    let mut config = TAGS.config.lock().expect("Failed to lock Config");
    let mut database = config.get(&location)
        .cloned()
        .unwrap_or_else(HashMap::new);
    if database.contains_key(&name) {
        return Err("Tag already exists.".to_owned());
    }

    database.insert(name.clone(),
                    Tag::new(name.clone(),
                             content,
                             message.author.id.0,
                             None,
                             Some(location.clone()),
                             None));
    config.insert(location, database);
    check_msg(context.say(&format!("Tag \"{}\" successfully created.", name)));

    Ok(())
}

pub fn info(context: &Context, message: &Message, args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();

    let name = match args.next() {
        Some(name) => name,
        None => return Err("Please specify a name for the tag to get info on.".to_owned()),
    };

    let name = name.trim().to_lowercase().to_owned();
    let guild_id = message.guild_id();
    let tag = TAGS.get_tag(guild_id, name)?;

    check_msg(context.send_message(message.channel_id, |m| m.embed(|e| tag.as_embed(e))));

    Ok(())
}

pub fn list(context: &Context, message: &Message, _args: Vec<String>) -> Result<(), String> {
    let guild_id = message.guild_id();
    let mut tags = TAGS.get_possible_tags(guild_id);
    let mut tags = tags.drain()
        .map(|(k, _)| k)
        .collect::<Vec<String>>();
    tags.sort();

    let response = if tags.is_empty() {
        "No tags available.".to_owned()
    } else {
        format!("Available tags: {}", tags.join(", "))
    };
    check_msg(context.say(&response));

    Ok(())
}

pub fn edit(context: &Context, message: &Message, args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();

    let name = match args.next() {
        Some(name) => name,
        None => return Err("Please specify a tag to edit.".to_owned()),
    };

    let name = name.trim().to_lowercase().to_owned();

    let guild_id = message.guild_id();
    let mut tag = match TAGS.get_tag(guild_id, name.clone()) {
        Ok(tag) => tag,
        Err(err) => return Err(err),
    };

    if !owner_check(message, &tag) {
        return Err("You do not have permission to do that.".to_owned());
    }

    let content = args.collect::<Vec<String>>();
    let content = if content.is_empty() {
        return Err("Please specify some content for the tag.".to_owned());
    } else {
        content.join(" ")
    };

    tag.content = content;
    TAGS.put_tag(guild_id, name.clone(), tag);

    check_msg(context.say(&format!("Tag \"{}\" successfully updated.", name)));

    Ok(())
}

pub fn delete(context: &Context, message: &Message, args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();

    let name = match args.next() {
        Some(name) => name,
        None => return Err("Please specify a tag to delete.".to_owned()),
    };

    let name = name.trim().to_lowercase().to_owned();

    let guild_id = message.guild_id();
    let tag = match TAGS.get_tag(guild_id, name.clone()) {
        Ok(tag) => tag,
        Err(err) => return Err(err),
    };

    if !owner_check(message, &tag) {
        return Err("You do not have permission to do that.".to_owned());
    }

    TAGS.delete_tag(guild_id, &name);

    check_msg(context.say(&format!("Tag \"{}\" successfully deleted.", name)));

    Ok(())
}

// Denies certain tag names from being used as keys.
fn verify_tag_name(name: &str) -> Result<(), String> {
    if name.contains("@everyone") || name.contains("@here") {
        return Err("Tag contains blocked words".to_owned());
    }

    if name.len() > 100 {
        return Err("Tag name limit is 100 characters".to_owned());
    }

    Ok(())
}

fn owner_check(message: &Message, tag: &Tag) -> bool {
    message.author.id == tag.owner_id
}

fn get_database_location(guild: Option<GuildId>) -> String {
    guild.map(|g| g.to_string())
        .unwrap_or_else(|| "generic".to_owned())
}
