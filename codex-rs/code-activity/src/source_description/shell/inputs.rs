use super::AtomInput;
use super::ChildSource;
use super::Result;
use super::State;
use super::UNSUPPORTED;
use super::Word;
use super::WordOrigin;
use super::input;
use super::redirections;

impl State<'_> {
    pub(super) fn atom_input(
        &self,
        words: &[Word],
        heredoc: Option<&redirections::Heredoc<'_>>,
        direct_only: bool,
    ) -> Result<AtomInput> {
        self.budget.check()?;
        let executable_word = words.first().ok_or(UNSUPPORTED)?;
        if executable_word.origin == WordOrigin::DecimalArgument {
            return Err(UNSUPPORTED);
        }
        let executable = &executable_word.value;
        let name = executable.rsplit('/').next().ok_or(UNSUPPORTED)?;
        if executable == "cd" {
            if heredoc.is_some() {
                return Err(UNSUPPORTED);
            }
            let target = match words.get(1..).ok_or(UNSUPPORTED)? {
                [target] if !target.value.starts_with('-') => target,
                [separator, target] if separator.value == "--" && target.value != "-" => target,
                _ => return Err(UNSUPPORTED),
            };
            if target.origin == WordOrigin::DecimalArgument {
                return Err(UNSUPPORTED);
            }
            return Ok(AtomInput::Directory {
                target: target.value.clone(),
                evidence: target.evidence.clone(),
            });
        }
        if direct_only && matches!(name, "env" | "command" | "uv" | "exec") {
            return Err(UNSUPPORTED);
        }
        let wrapper_offset = usize::from(executable == "exec");
        let capital_python = words
            .get(wrapper_offset)
            .is_some_and(|word| word.value.rsplit('/').next() == Some("Python"));
        let carrier = capital_python
            || wrapper_offset == 1
            || input::language(name).is_some()
            || matches!(name, "env" | "command" | "uv");
        if !carrier {
            if !ordinary(name) {
                return Err(UNSUPPORTED);
            }
            return match heredoc {
                None => Ok(AtomInput::Ordinary),
                Some(doc) if name == "cat" => Ok(AtomInput::LiteralData {
                    evidence: doc.evidence.clone(),
                }),
                _ => Err(UNSUPPORTED),
            };
        }
        let mut resolver_words: Vec<_> =
            words.iter().map(|word| Some(word.value.clone())).collect();
        if capital_python {
            *resolver_words.get_mut(wrapper_offset).ok_or(UNSUPPORTED)? = Some("python".to_owned());
        }
        if wrapper_offset == 1 {
            let child = resolver_words
                .get(1)
                .and_then(Option::as_deref)
                .ok_or(UNSUPPORTED)?;
            if input::language(child).is_none() {
                return Err(UNSUPPORTED);
            }
        }
        let invocation = input::resolve(resolver_words.get(wrapper_offset..).ok_or(UNSUPPORTED)?)
            .ok_or(UNSUPPORTED)?;
        if invocation.context_unknown {
            return Err(UNSUPPORTED);
        }
        let (source, evidence) = match invocation.input {
            input::Input::Inline { index, prefix } if heredoc.is_none() => {
                let index = index.checked_add(wrapper_offset).ok_or(UNSUPPORTED)?;
                let word = words.get(index).ok_or(UNSUPPORTED)?;
                if word.origin == WordOrigin::DecimalArgument {
                    return Err(UNSUPPORTED);
                }
                (
                    word.value.get(prefix..).ok_or(UNSUPPORTED)?.to_owned(),
                    word.evidence.clone(),
                )
            }
            input::Input::Stdin => {
                let doc = heredoc.ok_or(UNSUPPORTED)?;
                (
                    self.source
                        .get(doc.evidence.start_byte..doc.evidence.end_byte)
                        .ok_or(UNSUPPORTED)?
                        .to_owned(),
                    doc.evidence.clone(),
                )
            }
            _ => return Err(UNSUPPORTED),
        };
        Ok(AtomInput::Code(ChildSource {
            language: invocation.language,
            source,
            evidence,
        }))
    }
}

// Finite spelling vocabulary, unchanged from the preceding candidate. Literal
// arguments remain opaque; build recipes and ordinary commands can have effects.
fn ordinary(name: &str) -> bool {
    matches!(
        name,
        "cat"
            | "head"
            | "tail"
            | "ls"
            | "pwd"
            | "echo"
            | "printf"
            | "rg"
            | "mkdir"
            | "rm"
            | "cp"
            | "mv"
            | "touch"
            | "chmod"
            | "git"
            | "cargo"
            | "rustc"
            | "rustfmt"
            | "clippy-driver"
            | "just"
            | "sed"
            | "grep"
            | "find"
            | "wc"
            | "sort"
            | "uniq"
            | "diff"
            | "stat"
            | "file"
            | "du"
            | "date"
            | "uname"
            | "which"
            | "whoami"
            | "realpath"
            | "basename"
            | "dirname"
            | "readlink"
            | "cut"
            | "tr"
            | "tee"
            | "less"
            | "more"
    )
}
