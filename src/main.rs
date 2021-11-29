use ferris_says;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serenity::{
    async_trait,
    model::{
        channel::{Reaction, ReactionType},
        gateway::Ready,
        id::{ChannelId, GuildId, RoleId, UserId},
        interactions::{
            application_command::{
                ApplicationCommandInteractionDataOptionValue, ApplicationCommandOptionType,
            },
            Interaction, InteractionResponseType,
        },
    },
    prelude::*,
};
use std::{
    collections::HashMap,
    env,
    sync::{Arc, RwLock},
};

const BOT_CONFIG_NAME: &str = "bot_config";

#[derive(Serialize, Deserialize)]
struct BotData {
    name: String,

    data: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
struct UserConfig {
    reaction_roles: HashMap<String, String>,
}

struct Handler {
    guild_id: u64,
    reaction_roles: RwLock<HashMap<String, RoleId>>, // Emoji
}

impl Handler {
    fn new(guild_id: u64) -> Self {
        Handler {
            guild_id,
            reaction_roles: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        // let guild_id = env::var("GUILD_ID")
        //     .expect("Expected a guild id in the environment")
        //     .parse::<u64>()
        //     .expect("Guild id is not a valid id");

        let guild = GuildId(self.guild_id);

        let guild_channels = guild
            .channels(&ctx.http)
            .await
            .expect("Unable to get guild channels");

        let bot_data_channel_id: ChannelId = env::var("BOT_DATA_CHANNEL")
            .expect("Could not find BOT_DATA_CHANNEL env var")
            .parse::<u64>()
            .expect("Unable to parse as u64")
            .into();

        let admin_id: UserId = env::var("ADMIN_ID")
            .expect("Could not find ADMIN_ID env var")
            .parse::<u64>()
            .expect("Unable to parse as u64")
            .into();

        let bot_data_channel = &guild_channels[&bot_data_channel_id];
        let bot_data_channel_messages = bot_data_channel
            .messages(&ctx.http, |retriever| retriever.limit(50))
            .await
            .expect("Unable to get messages for bot data channel");

        // Only looking for user config
        for m in bot_data_channel_messages {
            if m.author.id != admin_id {
                continue;
            }

            let data: UserConfig =
                serde_json::from_str(&m.content).expect("Cannot read bot data content");

            // Read in guild roles
            let guild_roles = guild
                .roles(&ctx.http)
                .await
                .expect("Cannot get guild roles");

            let mut rr_store = self
                .reaction_roles
                .write()
                .expect("Unable to obtain lock on reaction role struct");

            for role in guild_roles.values() {
                let emoji = data.reaction_roles.get(&role.name);
                if let Some(e) = emoji {
                    rr_store.insert(e.to_string(), role.id);
                }
            }
        }

        println!("{:#?}", self.reaction_roles);

        // let guild_channels = guild
        //     .channels(&ctx.http)
        //     .await
        //     .context("Unable to get guild channels");

        let guild_commands = guild
            .set_application_commands(&ctx.http, |commands| {
                commands
                    .create_application_command(|command| {
                        command.name("ping").description("Ping the bot!")
                    })
                    .create_application_command(|command| {
                        command
                            .name("id")
                            .description("Get the id of a user")
                            .create_option(|option| {
                                option
                                    .name("id")
                                    .description("The user to lookup")
                                    .kind(ApplicationCommandOptionType::User)
                                    .required(true)
                            })
                    })
                    .create_application_command(|command| {
                        command
                            .name("ferris-say")
                            .description("Have a crab say something")
                            .create_option(|option| {
                                option
                                    .name("text")
                                    .description("The thing to say")
                                    .kind(ApplicationCommandOptionType::String)
                                    .required(true)
                            })
                    })
            })
            .await;

        println!(
            "I created the following guild command: {:#?}",
            guild_commands
        );
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            let content = match command.data.name.as_str() {
                "ping" => "pong".to_string(),
                "id" => {
                    let options = command
                        .data
                        .options
                        .get(0)
                        .expect("Expected user option")
                        .resolved
                        .as_ref()
                        .expect("Expected user object");

                    if let ApplicationCommandInteractionDataOptionValue::User(user, _member) =
                        options
                    {
                        format!("{}'s id is {}", user.tag(), user.id)
                    } else {
                        "Please provide a valid user".to_string()
                    }
                }
                "ferris-say" => {
                    let text = command
                        .data
                        .options
                        .get(0)
                        .expect("Expected something to say")
                        .resolved
                        .as_ref()
                        .expect("Expected text");

                    if let ApplicationCommandInteractionDataOptionValue::String(text) = text {
                        let mut buf = vec![];
                        ferris_says::say(text.as_bytes(), 26, &mut buf)
                            .expect("Cannot call ferris_says");

                        format!(
                            "```{}```",
                            String::from_utf8(buf).expect("Cannot create string from buffer")
                        )
                    } else {
                        "```OwO```".to_string()
                    }
                }
                _ => "not implemented :(".to_string(),
            };

            if let Err(why) = command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content(content))
                })
                .await
            {
                println!("Cannot respond to slash command: {}", why);
            }
        }
    }

    async fn reaction_add(&self, ctx: Context, add_reaction: Reaction) {
        let user_id = add_reaction
            .user(&ctx.http)
            .await
            .expect("Unable to get user");
        match add_reaction.emoji {
            ReactionType::Unicode(v) => {
                let guild = GuildId(self.guild_id);
                let member = guild.member(&ctx.http, user_id).await;
                if member.is_err() {
                    return;
                }
                let role_id: Option<RoleId>;
                {
                    let rr = self
                        .reaction_roles
                        .read()
                        .expect("Unable to access reaction role store");
                    role_id = rr.get(&v).cloned();
                }
                if let Some(ri) = role_id {
                    member
                        .unwrap()
                        .to_owned()
                        .add_role(&ctx.http, ri)
                        .await
                        .unwrap();
                }
            }
            // ReactionType::Custom { animated, id, name } => "🍆".to_string(),
            _ => {}
        };
    }

    async fn reaction_remove(&self, ctx: Context, removed_reaction: Reaction) {
        let user_id = removed_reaction
            .user(&ctx.http)
            .await
            .expect("Unable to get user");
        match removed_reaction.emoji {
            ReactionType::Unicode(v) => {
                let guild = GuildId(self.guild_id);
                let member = guild.member(&ctx.http, user_id).await;
                if member.is_err() {
                    return;
                }
                let role_id: Option<RoleId>;
                {
                    let rr = self
                        .reaction_roles
                        .read()
                        .expect("Unable to access reaction role store");
                    role_id = rr.get(&v).cloned();
                }
                if let Some(ri) = role_id {
                    member
                        .unwrap()
                        .to_owned()
                        .remove_role(&ctx.http, ri)
                        .await
                        .unwrap();
                }
            }
            // ReactionType::Custom { animated, id, name } => "🍆".to_string(),
            _ => {}
        };
    }
}

#[tokio::main]
async fn main() {
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // The Application Id is usually the Bot User Id.
    let application_id: u64 = env::var("APPLICATION_ID")
        .expect("Expected an application id in the environment")
        .parse()
        .expect("application id is not a valid id");

    let guild_id = env::var("GUILD_ID")
        .expect("Expected a guild id in the environment")
        .parse::<u64>()
        .expect("Guild id is not a valid id");

    let handler = Handler::new(guild_id);

    // Build our client.
    let mut client = Client::builder(token)
        .event_handler(handler)
        .application_id(application_id)
        .await
        .expect("Error creating client");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}
