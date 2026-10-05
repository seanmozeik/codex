//! Argument ownership for supported source carriers; never search past a child command.
use crate::Language;

#[derive(Clone, Copy)]
pub(super) enum Input {
    Stdin,
    Inline { index: usize, prefix: usize },
}
pub(super) struct Invocation {
    pub language: Language,
    pub input: Input,
    pub context_unknown: bool,
}

fn word(words: &[Option<String>], index: usize) -> Option<&str> {
    words.get(index)?.as_deref()
}

pub(super) fn language(name: &str) -> Option<Language> {
    let name = name.rsplit('/').next()?;
    if matches!(name, "sh" | "bash" | "zsh" | "dash") {
        return Some(Language::Shell);
    }
    if matches!(name, "node" | "nodejs" | "bun" | "deno" | "tsx" | "ts-node") {
        return Some(Language::TypeScript);
    }
    for prefix in ["python", "pypy"] {
        if let Some(version) = name.strip_prefix(prefix)
            && (version.is_empty() || version_number(version))
        {
            return Some(Language::Python);
        }
    }
    None
}

fn version_number(version: &str) -> bool {
    let version = version.strip_suffix('t').unwrap_or(version);
    let mut parts = version.split('.');
    matches!(parts.next(), Some("2" | "3"))
        && parts.all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn uv_flags(words: &[Option<String>], index: &mut usize) -> Option<()> {
    while let Some(token) = word(words, *index) {
        match token {
            "--no-sync"
            | "--locked"
            | "--frozen"
            | "--offline"
            | "--no-project"
            | "--no-config"
            | "--no-python-downloads"
            | "--quiet"
            | "-q"
            | "--verbose"
            | "-v" => *index += 1,
            "--python" | "-p" => {
                if !version_number(word(words, *index + 1)?) {
                    return None;
                }
                *index += 2;
            }
            _ if token.starts_with("--python=") => {
                if !version_number(token.strip_prefix("--python=")?) {
                    return None;
                }
                *index += 1;
            }
            _ => break,
        }
    }
    Some(())
}

pub(super) fn resolve(words: &[Option<String>]) -> Option<Invocation> {
    let mut index = 0;
    let mut context_unknown = false;
    for _ in 0..8 {
        let name = word(words, index)?.rsplit('/').next()?;
        index += 1;
        match name {
            "command" => {
                while matches!(word(words, index), Some("-p" | "--")) {
                    index += 1;
                }
            }
            "env" => {
                while let Some(token) = word(words, index) {
                    if token == "--" {
                        index += 1;
                        break;
                    }
                    if token.starts_with("--chdir=") || token.contains('=') {
                        context_unknown = true;
                        index += 1;
                    } else {
                        break;
                    }
                }
            }
            "uv" => {
                uv_flags(words, &mut index)?;
                if word(words, index)? != "run" {
                    return None;
                }
                index += 1;
                uv_flags(words, &mut index)?;
                if word(words, index) == Some("--") {
                    index += 1;
                }
                if word(words, index) == Some("-") && index + 1 == words.len() {
                    return Some(Invocation {
                        language: Language::Python,
                        input: Input::Stdin,
                        context_unknown,
                    });
                }
            }
            _ => {
                let language = language(name)?;
                let input = source_input(words, index, name, language)?;
                return Some(Invocation {
                    language,
                    input,
                    context_unknown,
                });
            }
        }
    }
    None
}

fn source_input(
    words: &[Option<String>],
    mut index: usize,
    name: &str,
    language: Language,
) -> Option<Input> {
    if name == "deno" {
        if word(words, index)? != "eval" {
            return None;
        }
        index += 1;
        while matches!(word(words, index), Some("-p" | "--print")) {
            index += 1;
        }
        word(words, index)?;
        return (index + 1 == words.len()).then_some(Input::Inline { index, prefix: 0 });
    }
    while let Some(flag) = word(words, index) {
        let pass = if language == Language::Python {
            matches!(flag, "-I" | "-S" | "-E" | "-s" | "-B" | "-u" | "-q")
        } else {
            matches!(
                flag,
                "--input-type=module" | "--input-type=commonjs" | "--no-warnings"
            )
        };
        if !pass {
            break;
        }
        index += 1;
    }
    let Some(flag) = word(words, index) else {
        return (index == words.len()).then_some(Input::Stdin);
    };
    if flag == "-" && index + 1 == words.len() {
        return Some(Input::Stdin);
    }
    let flags: &[&str] = match language {
        Language::Python => &["-c"],
        Language::Shell => &["-c", "-lc"],
        Language::TypeScript => &["-e", "--eval", "-p", "--print"],
    };
    if flags.contains(&flag) && index + 2 == words.len() {
        word(words, index + 1)?;
        return Some(Input::Inline {
            index: index + 1,
            prefix: 0,
        });
    }
    let prefixes: &[&str] = match language {
        Language::Python => &["-c"],
        Language::Shell => &[],
        Language::TypeScript => &["-e", "-p", "--eval=", "--print="],
    };
    let prefix = prefixes.iter().find(|prefix| flag.starts_with(**prefix))?;
    (index + 1 == words.len()).then_some(Input::Inline {
        index,
        prefix: prefix.len(),
    })
}
