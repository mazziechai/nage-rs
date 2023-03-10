use anyhow::{Result, anyhow};
use clap::Parser;

use crate::{core::{player::Player, prompt::Prompt as PromptUtil, manifest::Manifest, text::{Translations, TextContext}, choice::Notes, resources::{UnlockedInfoPages, InfoPages, Resources}, audio::Audio}, game::{gloop::GameLoopResult}, loading::saves::SaveManager};

#[derive(Parser, Debug, PartialEq)]
#[command(multicall = true)]
pub enum RuntimeCommand {
	#[command(about = "Try going back a choice")]
	Back,
	#[command(about = "Manage the display language")]
	Lang,
	#[command(about = "Display an info page")]
	Info,
	#[command(about = "Display an action log page")]
	Log,
	#[command(about = "Manage sound effects and music channels")]
	Sound,
	#[command(about = "Save the player data")]
	Save,
	#[command(about = "Save and quits the game")]
	Quit,
	#[command(about = "Display debug info about a prompt", hide = true)]
	Prompt,
	#[command(about = "List the currently applied notes", hide = true)]
	Notes,
	#[command(about = "List the currently applied variable names and their values", hide = true)]
	Variables,
}

/// The result of a runtime command.
pub enum CommandResult {
	/// Returns an input loop result to the original input call.
	Submit(GameLoopResult),
	/// Outputs a specified string and submits [`Retry`](InputLoopResult::Retry).
	Output(String)
}

impl CommandResult {
	pub fn retry() -> CommandResult {
		Self::Submit(GameLoopResult::Retry(true))
	}
}

impl RuntimeCommand {
	/// Determines if this command is allowed in a default, non-debug environment.
	fn is_normal(&self) -> bool {
		use RuntimeCommand::*;
		match self {
			Back | Lang | Info | Log | Sound | Save | Quit => true,
			_ => false
		}
	}

	/// Handles a [`Back`](RuntimeCommand::Back) command.
	fn back(player: &mut Player) -> Result<CommandResult> {
		if player.history.len() <= 1 {
			return Err(anyhow!("Can't go back right now!"));
		}
		player.back()?;
		Ok(CommandResult::Submit(GameLoopResult::Continue))
	}

	/// Handles a [`Lang`](RuntimeCommand::Lang) command.
	fn lang(player: &mut Player, translations: &Translations) -> Result<CommandResult> {
		if translations.is_empty() {
			return Err(anyhow!("No display languages loaded"));
		}

		println!();

		let lang_question = requestty::Question::select("Select a language")
			.choices(translations.keys())
			.build();
		let lang_choice = requestty::prompt_one(lang_question)?;
		player.lang = lang_choice.as_list_item().unwrap().text.clone();

		Ok(CommandResult::retry())
	}

	/// Handles an [`Info`](RuntimeCommand::Info) command.
	fn info(unlocked_pages: &UnlockedInfoPages, pages: &InfoPages) -> Result<CommandResult> {
		if unlocked_pages.is_empty() {
			return Err(anyhow!("No info pages unlocked"))
		}

		println!();
		
		let info_question = requestty::Question::select("Select an info page")
			.choices(unlocked_pages)
			.build();
		let info_choice = requestty::prompt_one(info_question)?;

		println!();
		termimad::print_text(pages.get(&info_choice.as_list_item().unwrap().text).unwrap());

		Ok(CommandResult::retry())
	}

	/// Handles a [`Log`](RuntimeCommand::Log) command.
	fn log(log: &Vec<String>) -> Result<CommandResult> {
		if log.is_empty() {
			return Err(anyhow!("No log entries to display"))
		}

		println!();

		let pages: Vec<&[String]> = log.chunks(5).collect();
		let page_choices: Vec<String> = pages.iter()
			.map(|chunk| chunk[0][..25].to_owned())
			.map(|line| format!("{line}..."))
			.collect();
		let page_question = requestty::Question::raw_select("Log page")
			.choices(page_choices)
			.build();
		let page_choice = requestty::prompt_one(page_question)?;

		let page_content = pages.get(page_choice.as_list_item().unwrap().index).unwrap();
		let entries = page_content.join("\n\n");
		Ok(CommandResult::Output(format!("\n{entries}")))
	}

	/// Handles a [`Sound`](RuntimeCommand::Sound) command.
	fn sound(player: &mut Player, audio_res: &Option<Audio>) -> Result<CommandResult> {
		let audio = audio_res.as_ref()
			.ok_or(anyhow!("No sound channels loaded"))?;

		println!();

		// Multi-selection where selected represents the channel being enabled and vice versa
		let channel_data: Vec<(String, bool)> = audio.players.keys()
    		.map(|channel| (channel.clone(), player.channels.contains(channel)))
    		.collect();
		let channel_selection = requestty::Question::multi_select("Select sound channels")
    		.choices_with_default(channel_data)
    		.build();
		let channel_choices = requestty::prompt_one(channel_selection)?;

		// The selected channels
		let enabled_channels: Vec<String> = channel_choices.as_list_items().unwrap().iter()
    		.map(|choice| choice.text.clone())
			.collect();

		// Each possible channel will either be selected or not; if so, append to player's
		// enabled channel list if not already present, otherwise remove and stop the channel playback if necessary
		for channel in audio.players.keys() {
			if enabled_channels.contains(channel) {
				player.channels.insert(channel.clone());
			}
			else {
				player.channels.remove(channel);
				audio.get_player(channel)?.stop();
			}
		}

		Ok(CommandResult::retry())
	}

	/// Handles a [`Prompt`](RuntimeCommand::Prompt) command.
	fn prompt(notes: &Notes, resources: &Resources, text_context: &TextContext) -> Result<CommandResult> {
		println!();

		let file_question = requestty::Question::select("Prompt file")
			.choices(resources.prompts.keys())
			.build();
		let file_choice = requestty::prompt_one(file_question)?;
		let file = &file_choice.as_list_item().unwrap().text;

		let prompt_question = requestty::Question::select(format!("Prompt in '{}'", file))
			.choices(PromptUtil::get_file(&resources.prompts, file)?.keys())
			.build();
		let prompt_choice = requestty::prompt_one(prompt_question)?;
		let prompt_name = &prompt_choice.as_list_item().unwrap().text;

		let prompt = PromptUtil::get(&resources.prompts, prompt_name, file)?;
		Ok(CommandResult::Output(prompt.debug_info(prompt_name, file, &resources.prompts, notes, text_context)?))
	}

	/// Handles a [`Notes`](RuntimeCommand::Notes) command.
	fn notes(player: &Player) -> Result<CommandResult> {
		if player.notes.is_empty() {
			return Err(anyhow!("No notes applied"))
		}
		let result = itertools::join(&player.notes, ", ");
		Ok(CommandResult::Output(result))
	}
	
	/// Handles a [`Variables`](RuntimeCommand::Variables) command.
	fn variables(player: &Player) -> Result<CommandResult> {
		if player.variables.is_empty() {
			return Err(anyhow!("No variables applied"))
		}
		let vars = player.variables.clone().into_iter()
			.map(|(name, value)| format!("{name}: {value}"))
			.collect::<Vec<String>>()
			.join("\n");
		Ok(CommandResult::Output(format!("\n{vars}")))
	}

	/// Executes a runtime command if the player has permission to do so.
	///
	/// Any errors will be reported to the input loop with a retry following.
	pub fn run(&self, config: &Manifest, player: &mut Player, saves: &SaveManager, resources: &Resources, text_context: &TextContext) -> Result<CommandResult> {
		if !self.is_normal() && !config.settings.debug {
			return Err(anyhow!("Unable to access debug commands"));
		}
		use RuntimeCommand::*;
		use CommandResult::*;
		let result = match self {
			Back => Self::back(player)?,
			Lang => Self::lang(player, &resources.translations)?,
			Info => Self::info(&player.info_pages, &resources.info_pages)?,
			Log => Self::log(&player.log)?,
			Sound => Self::sound(player, &resources.audio)?,
			Save => {
				saves.write(player, None, false)?;
				Output("Saving... ".to_owned())
			}
			Quit => Submit(GameLoopResult::Shutdown(false)),
			Prompt => Self::prompt(&player.notes, resources, text_context)?,
			Notes => Self::notes(player)?,
			Variables => Self::variables(player)?
		};
		Ok(result)
	}
}