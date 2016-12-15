// Copyright (c) 2016 Nikita Pekin and the smexybot contributors
// See the README.md file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Provides the functionality for a `!fuyu` command, which first reads in a
//! double newline delimited file, then uses that to generate a
//! [Markov chain][markov-chain] out of the data using the [markov][markov-lib]
//! library. It then returns a message generated by the Markov chain.
//!
//! [markov-lib]: https://github.com/aatxe/markov
//! [markov-chain]: https://en.wikipedia.org/wiki/Markov_chain

extern crate markov;

use self::markov::Chain;
use serenity::client::Context;
use serenity::model::Message;

use util::{check_msg, random_colour};

pub fn handler(context: &Context, _message: &Message, _args: Vec<String>)
    -> Result<(), String>
{
    let channel_id = context.channel_id.expect("Failed to retrieve channel ID from context");
    // TODO: handle this properly.
    if let Err(err) = context.broadcast_typing(channel_id) {
        return Err(format!("{:?}", err));
    }

    let response = create_chain().generate_str();
    let colour = random_colour();
    check_msg(context.send_message(
        channel_id,
        |m| m.embed(|e| e.colour(colour).description(response.as_ref())),
    ));
    Ok(())
}

fn create_chain() -> Chain<String> {
    let chat_logs = load_chat_logs();
    let mut chain = Chain::new();
    for line in chat_logs.split("\n\n") {
        chain.feed_str(line);
    }
    chain
}

#[cfg(feature = "fuyu-include")]
fn load_chat_logs() -> String {
    const FUYU_CHAT_LOGS: &'static str = include_str!("../../logs/fuyu.txt");

    FUYU_CHAT_LOGS.to_owned()
}

#[cfg(not(feature = "fuyu-include"))]
fn load_chat_logs() -> String {
    use std::fs::File;
    use std::io::Read;

    const FILE_NAME: &'static str = "logs/fuyu.txt";

    let mut file = File::open(FILE_NAME).expect("Failed to open chat log file");
    let mut contents = String::new();
    file.read_to_string(&mut contents).expect("Failed to read chat log file");

    contents
}
