// Copyright (c) 2016 Nikita Pekin and the smexybot contributors
// See the README.md file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![cfg_attr(feature = "nightly", feature(proc_macro))]
#![cfg_attr(feature = "clippy", feature(plugin))]
#![cfg_attr(feature = "clippy", plugin(clippy))]
#![warn(missing_copy_implementations,
        missing_debug_implementations,
        missing_docs,
        trivial_casts,
        trivial_numeric_casts,
        unused_extern_crates,
        unused_import_braces)]
#![deny(missing_docs, non_camel_case_types, unsafe_code)]
#![cfg_attr(not(feature = "nightly"), deny(warnings))]
#![cfg_attr(feature="clippy", warn(
        cast_possible_truncation,
        cast_possible_wrap,
        cast_precision_loss,
        cast_sign_loss,
        mut_mut,
        wrong_pub_self_convention))]
// This allows us to use `unwrap` on `Option` values when compiling in test mode
// (because using it in tests is idiomatic).
#![cfg_attr(all(not(test), feature="clippy"), warn(result_unwrap_used))]

//! Smexybot is a general-purpose [Discord](https://discordapp.com/) bot written
//! in [Rust](https://www.rust-lang.org/). It is built upon the
//! [serenity.rs](https://github.com/zeyla/serenity.rs) Discord API.

extern crate chrono;
extern crate env_logger;
extern crate hyper;
#[cfg(any(feature = "roll", feature = "wolfram", feature = "xkcd"))]
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate rand;
extern crate serde;
#[cfg(feature = "nightly")]
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
#[macro_use]
extern crate serenity;
extern crate url;

mod command;
mod config;
mod counter;
mod error;
mod util;

use chrono::{DateTime, UTC};
use config::Config;
use counter::CommandCounter;
use serenity::Client;
use serenity::client::LoginType;
use serenity::ext::framework::Framework;
use serenity::model::UserId;
use std::collections::HashMap;
use std::env;
use util::{check_msg, timestamp_to_string};

const RATE_LIMIT_MESSAGE: &'static str = "Try this again in %time% seconds.";

lazy_static! {
    static ref CONFIG: Config = Config::new(Some("config.json"));
    static ref UPTIME: DateTime<UTC> = UTC::now();
}

fn main() {
    // Initialize the `env_logger` to provide logging output.
    env_logger::init().expect("Failed to initialize env_logger");

    // Initialize the `UPTIME` variable.
    debug!("Initialized at: {}", timestamp_to_string(&*UPTIME));

    // Create a client for a user.
    let (_, mut client) = login();

    {
        let mut data = client.data.lock().expect("Failed to lock client data");
        data.insert::<CommandCounter>(HashMap::default());
    }

    client.on_ready(|_context, ready| {
        let shard_info = if let Some(s) = ready.shard {
            Some(format!("shard {}/{} ", s[0] + 1, s[1]))
        } else {
            None
        };
        println!(
            "Started {}as {}#{}, serving {} guilds",
            shard_info.unwrap_or_else(|| "".to_owned()),
            ready.user.name,
            ready.user.discriminator,
            ready.guilds.len(),
        );
    });

    client.with_framework(build_framework);

    if let Err(err) = client.start_autosharded() {
        error!("Client error: {:?}", err);
    }
}

// Configures the `Framework` used by serenity, and registers the handlers for
// any enabled commands.
fn build_framework(framework: Framework) -> Framework {
    let mut framework = framework.configure(|c| {
            c.rate_limit_message(RATE_LIMIT_MESSAGE)
                .prefix(&CONFIG.command_prefix)
                .owners(CONFIG.owners.iter().map(|id| UserId(*id)).collect())
        })
        .before(|context, message, command_name| {
            info!(
                "Got command '{}' from user '{}'",
                command_name,
                message.author.name,
            );

            // Increment the number of times this command has been run. If the
            // command's name does not exist in the counter, add a default value of
            // 0.
            let mut data = context.data.lock().expect("Failed to lock context data");
            let counter = data.get_mut::<CommandCounter>().unwrap();
            let entry = counter.entry(command_name.clone()).or_insert(0);
            *entry += 1;

            true
        })
        .after(|context, _message, command_name, error| {
            if let Err(err) = error {
                check_msg(context.say(&err));
            } else {
                debug!("Processed command '{}'", command_name);
            }
        });

    #[cfg(feature = "fuyu")]
    {
        framework = framework.command("fuyu", |c| c.exec(command::fuyu::fuyu));
    }
    #[cfg(feature = "help")]
    {
        use serenity::ext::framework::help_commands;
        framework = framework.command("help", |c| c.exec_help(help_commands::plain));
    }
    #[cfg(feature = "ping")]
    {
        framework = framework.command("ping", |c| {
            c.desc("Responds with 'Pong', as well as a latency estimate.")
                .exec(command::ping::ping)
                .owners_only(true)
        });
    }
    #[cfg(feature = "roll")]
    {
        framework = framework.command("roll", |c| c.exec(command::roll::roll));
    }
    #[cfg(feature = "stats")]
    {
        framework = framework.command("stats", |c| c.exec(command::stats::stats));
    }
    #[cfg(feature = "tag")]
    {
        framework = framework.command("tag", |c| c.exec(command::tag::tag));
    }
    #[cfg(feature = "wolfram")]
    {
        framework = framework.command("wolfram", |c| c.exec(command::wolfram_alpha::wolfram));
    }
    #[cfg(feature = "xkcd")]
    {
        framework = framework.command("xkcd", |c| c.exec(command::xkcd::xkcd));
    }

    framework
}

// Creates a `Client`.
fn login() -> (LoginType, Client) {
    debug!("Attempting to login");

    if let Ok(bot_token) = env::var("DISCORD_BOT_TOKEN") {
        debug!("Performing bot token login");
        return (LoginType::Bot, Client::login_bot(&bot_token));
    }
    debug!("Skipping bot token login");

    if let Ok(user_token) = env::var("DISCORD_USER_TOKEN") {
        debug!("Performing user token login");
        return (LoginType::User, Client::login_user(&user_token));
    }
    debug!("Skipping user token login");

    panic!("No suitable authentication method found");
}
