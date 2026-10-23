use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

const AVATAR_PARAMETER_PREFIX: &str = "/avatar/parameters/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvatarContext {
    pub avatar_id: String,
    pub parameters: Vec<AvatarParameter>,
}

impl AvatarContext {
    pub fn parameter_by_canonical_address(&self, address: &str) -> Option<&AvatarParameter> {
        self.parameters.iter().find(|parameter| parameter.address == address)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvatarParameter {
    pub value_type: AvatarParameterType,
    pub name: String,
    pub address: String,
    pub aliases: Vec<String>,
}

impl AvatarParameter {
    pub fn runtime_addresses(&self) -> impl Iterator<Item = &str> {
        self.aliases
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(self.address.as_str()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvatarParameterType {
    Bool,
    Int,
    Float,
    String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct OscQueryNode {
    #[serde(rename = "FULL_PATH", default)]
    full_path: Option<String>,
    #[serde(rename = "TYPE", default)]
    value_type: Option<String>,
    #[serde(rename = "VALUE", default)]
    value: Option<Vec<serde_json::Value>>,
    #[serde(rename = "CONTENTS", default)]
    contents: Option<BTreeMap<String, OscQueryNode>>,
}

pub async fn discover_avatar_context() -> Result<Option<AvatarContext>> {
    oyasumivr_oscquery::client::init()
        .await
        .map_err(|error| anyhow!("initializing OSCQuery discovery: {error:?}"))?;

    let Some((host, port)) = oyasumivr_oscquery::client::get_vrchat_oscquery_address().await else {
        return Ok(None);
    };

    fetch_avatar_context(&host, port).await.map(Some)
}

async fn fetch_avatar_context(host: &str, port: u16) -> Result<AvatarContext> {
    let url = format!("http://{host}:{port}/");
    let node = reqwest::get(&url)
        .await
        .with_context(|| format!("fetching VRChat OSCQuery tree from {url}"))?
        .error_for_status()
        .with_context(|| format!("VRChat OSCQuery returned an error for {url}"))?
        .json::<OscQueryNode>()
        .await
        .with_context(|| format!("decoding VRChat OSCQuery tree from {url}"))?;

    parse_avatar_context(&node)
}

fn parse_avatar_context(root: &OscQueryNode) -> Result<AvatarContext> {
    let avatar = child(root, "avatar").context("OSCQuery tree has no /avatar node")?;
    let change = child(avatar, "change").context("OSCQuery tree has no /avatar/change node")?;
    let avatar_id = change
        .value
        .as_ref()
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .context("OSCQuery /avatar/change does not contain an avatar id")?
        .to_owned();

    let mut parameters = Vec::new();
    if let Some(parameter_root) = child(avatar, "parameters") {
        collect_parameters(parameter_root, &mut parameters);
    }
    parameters.sort_by(|a, b| a.address.cmp(&b.address));

    Ok(AvatarContext {
        avatar_id,
        parameters,
    })
}

fn child<'a>(node: &'a OscQueryNode, name: &str) -> Option<&'a OscQueryNode> {
    node.contents.as_ref()?.get(name)
}

fn collect_parameters(node: &OscQueryNode, out: &mut Vec<AvatarParameter>) {
    if let Some(contents) = node.contents.as_ref() {
        for child in contents.values() {
            collect_parameters(child, out);
        }
        return;
    }

    let Some(address) = node.full_path.as_deref() else {
        return;
    };
    let Some(name) = address.strip_prefix(AVATAR_PARAMETER_PREFIX) else {
        return;
    };
    let Some(value_type) = node
        .value_type
        .as_deref()
        .and_then(map_oscquery_type)
    else {
        return;
    };

    out.push(sanitize_parameter(AvatarParameter {
        value_type,
        name: name.to_owned(),
        address: address.to_owned(),
        aliases: Vec::new(),
    }));
}

fn map_oscquery_type(value: &str) -> Option<AvatarParameterType> {
    match value {
        "f" => Some(AvatarParameterType::Float),
        "i" => Some(AvatarParameterType::Int),
        "s" => Some(AvatarParameterType::String),
        "T" | "F" => Some(AvatarParameterType::Bool),
        _ => None,
    }
}

fn sanitize_parameter(parameter: AvatarParameter) -> AvatarParameter {
    sanitize_vrcfury_parameter(parameter.clone())
        .or_else(|| sanitize_modular_avatar_parameter(parameter.clone()))
        .unwrap_or(parameter)
}

fn sanitize_vrcfury_parameter(parameter: AvatarParameter) -> Option<AvatarParameter> {
    let bytes = parameter.name.as_bytes();
    if !parameter.name.starts_with("VF") {
        return None;
    }

    let mut index = 2;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 2 || bytes.get(index).copied() != Some(b'_') {
        return None;
    }

    canonicalize_parameter(parameter, index + 1)
}

fn sanitize_modular_avatar_parameter(parameter: AvatarParameter) -> Option<AvatarParameter> {
    let marker = "$$Internal_";
    let marker_index = parameter.name.rfind(marker)?;
    let suffix = &parameter.name[marker_index + marker.len()..];
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    canonicalize_parameter(parameter, 0).map(|mut canonical| {
        let canonical_name = canonical.name[..marker_index].to_owned();
        canonical.name = canonical_name.clone();
        canonical.address = format!("{AVATAR_PARAMETER_PREFIX}{canonical_name}");
        canonical
    })
}

fn canonicalize_parameter(mut parameter: AvatarParameter, prefix_len: usize) -> Option<AvatarParameter> {
    let runtime_address = parameter.address.clone();
    let canonical_name = parameter.name.get(prefix_len..)?.to_owned();
    if canonical_name.is_empty() {
        return None;
    }

    parameter.name = canonical_name.clone();
    parameter.address = format!("{AVATAR_PARAMETER_PREFIX}{canonical_name}");
    parameter.aliases.push(runtime_address);
    Some(parameter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(name: &str) -> AvatarParameter {
        AvatarParameter {
            value_type: AvatarParameterType::Bool,
            name: name.into(),
            address: format!("{AVATAR_PARAMETER_PREFIX}{name}"),
            aliases: Vec::new(),
        }
    }

    #[test]
    fn strips_vrcfury_runtime_prefix_but_keeps_alias() {
        let parameter = sanitize_parameter(parameter("VF123_Go/Stationary"));
        assert_eq!(parameter.name, "Go/Stationary");
        assert_eq!(parameter.address, "/avatar/parameters/Go/Stationary");
        assert_eq!(
            parameter.aliases,
            vec!["/avatar/parameters/VF123_Go/Stationary"]
        );
    }

    #[test]
    fn strips_modular_avatar_internal_suffix_but_keeps_alias() {
        let parameter = sanitize_parameter(parameter("Go/Stationary$$Internal_42"));
        assert_eq!(parameter.name, "Go/Stationary");
        assert_eq!(parameter.address, "/avatar/parameters/Go/Stationary");
        assert_eq!(
            parameter.aliases,
            vec!["/avatar/parameters/Go/Stationary$$Internal_42"]
        );
    }

    #[test]
    fn leaves_normal_parameters_untouched() {
        let parameter = sanitize_parameter(parameter("Go/Stationary"));
        assert_eq!(parameter.name, "Go/Stationary");
        assert_eq!(parameter.address, "/avatar/parameters/Go/Stationary");
        assert!(parameter.aliases.is_empty());
    }

    #[test]
    fn parses_avatar_id_and_parameter_tree() {
        let json = r#"
        {
          "CONTENTS": {
            "avatar": {
              "CONTENTS": {
                "change": { "VALUE": ["avtr_test"] },
                "parameters": {
                  "VF123_Go": {
                    "CONTENTS": {
                      "Stationary": {
                        "FULL_PATH": "/avatar/parameters/VF123_Go/Stationary",
                        "TYPE": "T",
                        "VALUE": [true]
                      }
                    }
                  }
                }
              }
            }
          }
        }
        "#;
        let node: OscQueryNode = serde_json::from_str(json).unwrap();
        let context = parse_avatar_context(&node).unwrap();

        assert_eq!(context.avatar_id, "avtr_test");
        assert_eq!(context.parameters.len(), 1);
        assert_eq!(context.parameters[0].address, "/avatar/parameters/Go/Stationary");
        assert_eq!(
            context.parameters[0].aliases,
            vec!["/avatar/parameters/VF123_Go/Stationary"]
        );
    }
}
