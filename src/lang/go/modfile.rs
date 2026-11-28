#[derive(Debug, Default, PartialEq)]
pub struct ModFile {
    pub module: Option<String>,
    pub requires: Vec<(String, String)>,
    pub replaces: Vec<(String, String)>,
}

pub fn read(text: &str) -> ModFile {
    let mut file = ModFile::default();
    let mut block: Option<String> = None;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        let tokens: Vec<&str> = (line.split_whitespace())
            .map(|t| t.trim_matches(|c| c == '"' || c == '`'))
            .collect();
        let (directive, args) = match (&block, tokens.as_slice()) {
            (Some(_), [")"]) => {
                block = None;
                continue;
            }
            (Some(directive), args) => (directive.clone(), args),
            (None, [directive, "("]) => {
                block = Some(directive.to_string());
                continue;
            }
            (None, [directive, args @ ..]) => (directive.to_string(), args),
            (None, []) => continue,
        };
        match (directive.as_str(), args) {
            ("module", [path, ..]) => file.module = Some(path.to_string()),
            ("require", [path, version, ..]) => {
                file.requires.push((path.to_string(), version.to_string()));
            }
            ("replace", args) => {
                let Some(arrow) = args.iter().position(|a| *a == "=>") else {
                    continue;
                };
                if let (Some(old), Some(new)) = (args.first(), args.get(arrow + 1))
                    && arrow > 0
                {
                    file.replaces.push((old.to_string(), new.to_string()));
                }
            }
            _ => {}
        }
    }
    file
}

pub struct Modules {
    modules: Vec<Module>,
}

struct Module {
    dir: String,
    path: Option<String>,
    replaces: Vec<(String, Option<String>)>,
}

impl Modules {
    pub fn new(configs: &[(&str, &[u8])]) -> Self {
        let modules = (configs.iter())
            .filter(|(path, _)| path.rsplit('/').next() == Some("go.mod"))
            .filter(|(_, bytes)| bytes.len() <= crate::lang::MAX_BYTES)
            .map(|(path, bytes)| {
                let dir = path.rsplit_once('/').map_or("", |(d, _)| d).to_string();
                let file = read(&String::from_utf8_lossy(bytes));
                let replaces = (file.replaces.into_iter())
                    .map(|(old, new)| {
                        let target = local(&dir, &new);
                        (old, target)
                    })
                    .collect();
                Module {
                    dir,
                    path: file.module,
                    replaces,
                }
            })
            .collect();
        Self { modules }
    }

    fn owner(&self, dir: &str) -> Option<&Module> {
        (self.modules.iter())
            .filter(|m| within(dir, &m.dir))
            .max_by_key(|m| m.dir.len())
    }

    pub fn below<'d>(&self, dir: &'d str) -> &'d str {
        self.owner(dir).map_or(dir, |m| &dir[m.dir.len()..])
    }

    pub fn dir(&self, from: &str, specifier: &str) -> Option<String> {
        let own = self.owner(from)?;
        let replaced = (own.replaces.iter())
            .filter_map(|(old, dir)| Some((inside(specifier, old)?, dir.as_deref(), old.len())))
            .max_by_key(|(_, _, len)| *len);
        let replaced = match replaced {
            Some((rest, dir, len)) => Some((rest, dir?, len)),
            None => None,
        };
        let found = replaced.or_else(|| {
            let matches: Vec<(&str, &str, usize)> = (self.modules.iter())
                .filter_map(|m| {
                    let path = m.path.as_deref()?;
                    Some((inside(specifier, path)?, m.dir.as_str(), path.len()))
                })
                .collect();
            let longest = matches.iter().map(|(_, _, len)| *len).max()?;
            match &matches
                .iter()
                .filter(|m| m.2 == longest)
                .collect::<Vec<_>>()[..]
            {
                [one] => Some(**one),
                _ => None,
            }
        });
        let (rest, base, _) = found?;
        if !rest.is_empty() && rest.split('/').any(|s| matches!(s, "" | "." | "..")) {
            return None;
        }
        let target = match (base, rest) {
            ("", rest) => rest.to_string(),
            (base, "") => base.to_string(),
            (base, rest) => format!("{base}/{rest}"),
        };
        (self.owner(&target)?.dir == base).then_some(target)
    }
}

pub fn unversioned(path: &str) -> String {
    let mut at = 0;
    while let Some(found) = path[at..].find("/v") {
        let start = at + found;
        let digits = (path[start + 2..].bytes())
            .take_while(u8::is_ascii_digit)
            .count();
        let (number, rest) = (
            &path[start + 2..start + 2 + digits],
            &path[start + 2 + digits..],
        );
        let major = !number.starts_with('0') && number.parse::<u32>().is_ok_and(|n| n >= 2);
        if major && (rest.is_empty() || rest.starts_with(['/', '.'])) {
            return format!("{}{rest}", &path[..start]);
        }
        at = start + 2;
    }
    path.to_string()
}

pub fn inside<'s>(specifier: &'s str, module: &str) -> Option<&'s str> {
    match specifier.strip_prefix(module)? {
        "" => Some(""),
        rest => rest.strip_prefix('/'),
    }
}

fn within(dir: &str, parent: &str) -> bool {
    parent.is_empty()
        || dir == parent
        || dir.strip_prefix(parent).is_some_and(|r| r.starts_with('/'))
}

pub fn local_target(target: &str) -> bool {
    target.starts_with("./") || target.starts_with("../") || matches!(target, "." | "..")
}

pub fn module_target(target: &str) -> bool {
    !target.starts_with(['/', '\\', '.']) && !target.contains([':', '\\'])
}

pub fn held_by(target: &str, identity: &str, manifest: &str) -> bool {
    let dir = manifest.rsplit_once('/').map_or("", |(d, _)| d);
    let lower = target.to_ascii_lowercase();
    let Some(rest) = inside(&lower, identity).map(|r| &target[target.len() - r.len()..]) else {
        return false;
    };
    let versioned = unversioned(&format!("_/{rest}"));
    rest == dir || versioned[1..].trim_start_matches('/') == dir
}

fn local(dir: &str, target: &str) -> Option<String> {
    let folded = local_target(target).then(|| crate::repo::relative(dir, target));
    folded
        .flatten()
        .filter(|p| !p.split('/').any(|s| s == ".."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_go_mod_gives_its_module_path_and_replaces() {
        let file = read(
            "// a comment\nmodule \"example.com/shop\" // trailing\n\ngo 1.22\n\n\
             require (\n\texample.com/billing v0.1.0\n)\n\
             replace example.com/billing => ./billing\n\
             replace (\n\texample.com/legacy v1.0.0 => ../legacy\n\texample.com/up => example.com/up v1.2.0\n)\n\
             replace broken =>\n",
        );
        assert_eq!(
            file,
            ModFile {
                module: Some("example.com/shop".into()),
                requires: vec![("example.com/billing".into(), "v0.1.0".into())],
                replaces: vec![
                    ("example.com/billing".into(), "./billing".into()),
                    ("example.com/legacy".into(), "../legacy".into()),
                    ("example.com/up".into(), "example.com/up".into()),
                ],
            }
        );
    }

    #[test]
    fn a_target_names_the_go_mod_of_the_repository_whose_identity_prefixes_it() {
        let id = "github.com/regen-network/protobuf";
        let held = |target: &str, manifest: &str| held_by(target, id, manifest);
        assert!(held("github.com/regen-network/protobuf", "go.mod"));
        assert!(held("github.com/Regen-Network/protobuf", "go.mod"));
        assert!(held("github.com/regen-network/protobuf/v2", "go.mod"));
        assert!(held("github.com/regen-network/protobuf/v2", "v2/go.mod"));
        assert!(held(
            "github.com/regen-network/protobuf/v2/sub",
            "sub/go.mod"
        ));
        assert!(held_by(
            "github.com/acme/v2",
            "github.com/acme/v2",
            "go.mod"
        ));
        assert!(held("github.com/regen-network/protobuf/sub", "sub/go.mod"));
        assert!(!held("github.com/regen-network/protobuf/sub", "go.mod"));
        assert!(!held("github.com/regen-network/protobuf", "sub/go.mod"));
        assert!(held(
            "github.com/Regen-Network/protobuf/SubDir",
            "SubDir/go.mod"
        ));
        assert!(!held(
            "github.com/regen-network/protobuf/SubDir",
            "subdir/go.mod"
        ));
        assert!(!held("github.com/regen-network/protobuf2", "go.mod"));
        assert!(!held("github.com/regen-network", "go.mod"));
        assert!(!held("github.com/other/protobuf", "go.mod"));
    }

    #[test]
    fn only_a_module_path_is_a_module_target() {
        assert!(module_target("github.com/a/b"));
        assert!(module_target("kit"));
        assert!(!module_target("/abs/dir"));
        assert!(!module_target("C:\\dir"));
        assert!(!module_target("./x"));
        assert!(!module_target("https://x"));
    }

    #[test]
    fn the_major_version_of_the_module_alone_is_dropped() {
        for (path, expected) in [
            ("github.com/labstack/echo/v4", "github.com/labstack/echo"),
            (
                "github.com/labstack/echo/v4.New",
                "github.com/labstack/echo.New",
            ),
            ("redis/go-redis/v9.NewClient", "redis/go-redis.NewClient"),
            (
                "confluent-kafka-go/v2/kafka.NewProducer",
                "confluent-kafka-go/kafka.NewProducer",
            ),
            ("example.com/x/v2/sub/v3/pkg", "example.com/x/sub/v3/pkg"),
            ("IBM/sarama.NewSyncProducer", "IBM/sarama.NewSyncProducer"),
            ("example.com/v1/x.New", "example.com/v1/x.New"),
            ("example.com/x/v0", "example.com/x/v0"),
            ("example.com/x/v02", "example.com/x/v02"),
            ("example.com/vendor/x.New", "example.com/vendor/x.New"),
            ("example.com/v2x/x.New", "example.com/v2x/x.New"),
            ("gopkg.in/yaml.v3", "gopkg.in/yaml.v3"),
            ("net/http", "net/http"),
        ] {
            assert_eq!(unversioned(path), expected, "{path}");
        }
    }

    #[test]
    fn a_go_mod_past_the_size_limit_is_invalid() {
        let head = "module example.com/shop\n//";
        let line = "\nreplace example.com/billing => ./billing-v2\n";
        let cut = crate::lang::MAX_BYTES + 1
            - head.len()
            - "\nreplace example.com/billing => ./bil".len();
        let at = |pad: usize, line: &str| format!("{head}{}{line}", " ".repeat(pad));
        let cut_file = at(cut, line)[..crate::lang::MAX_BYTES + 1].to_string();
        assert!(cut_file.ends_with("./bil"));
        let billing: &[u8] = b"module example.com/billing\n";
        let dir = |text: &str| {
            let configs = [
                ("go.mod", text.as_bytes()),
                ("bil/go.mod", billing),
                ("billing-v2/go.mod", billing),
            ];
            Modules::new(&configs).dir("", "example.com/billing")
        };
        assert_eq!(dir(&cut_file), None);
        assert_eq!(dir(&at(0, line)).as_deref(), Some("billing-v2"));
    }

    #[test]
    fn a_replace_names_a_local_directory_inside_the_repository_alone() {
        assert_eq!(local("", "./billing").as_deref(), Some("billing"));
        assert_eq!(local("app", "../lib/x").as_deref(), Some("lib/x"));
        assert_eq!(local("app", "./").as_deref(), Some("app"));
        assert_eq!(local("", "../outside"), None);
        assert_eq!(local("", "/abs/path"), None);
        assert_eq!(local("", "example.com/up"), None);
    }

    #[test]
    fn an_import_names_the_directory_its_module_owns() {
        let configs: &[(&str, &[u8])] = &[
            (
                "go.mod",
                b"module example.com/shop\nreplace example.com/billing => ./billing\n",
            ),
            ("billing/go.mod", b"module example.com/billing\n"),
            ("worker/go.mod", b"module example.com/worker\n"),
            ("forks/billing/go.mod", b"module example.com/billing\n"),
            ("forks/go.mod", b"module example.com/forks\n"),
        ];
        let modules = Modules::new(configs);
        let dir = |from, spec| modules.dir(from, spec);
        assert_eq!(dir("", "example.com/shop/store").as_deref(), Some("store"));
        assert_eq!(dir("cmd", "example.com/shop").as_deref(), Some(""));
        assert_eq!(
            dir("", "example.com/billing/tax").as_deref(),
            Some("billing/tax")
        );
        assert_eq!(dir("forks", "example.com/forks").as_deref(), Some("forks"));
        assert_eq!(dir("", "example.com/shop/worker/jobs"), None);
        assert_eq!(
            dir("worker", "example.com/worker/jobs").as_deref(),
            Some("worker/jobs")
        );
        let forked: &[(&str, &[u8])] = &[
            ("go.mod", b"module example.com/shop\nreplace example.com/billing v1.0.0 => example.com/fork v1.0.0\n"),
            ("billing/go.mod", b"module example.com/billing\n"),
        ];
        let forked = Modules::new(forked);
        assert_eq!(forked.dir("", "example.com/billing/tax"), None);
        assert_eq!(
            forked.dir("billing", "example.com/billing/tax").as_deref(),
            Some("billing/tax")
        );
        assert_eq!(dir("worker", "example.com/billing/tax"), None);
        assert_eq!(dir("worker", "example.com/forks").as_deref(), Some("forks"));
        assert_eq!(dir("", "fmt"), None);
        assert_eq!(dir("", "example.com/shopping"), None);
        assert_eq!(dir("", "example.com/shop/../x"), None);
    }
}
