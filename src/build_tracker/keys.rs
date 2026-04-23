//! Canonical artefact keys for the build tracker.
//!
//! One scheme used by both the tracker (population from prepared YAML) and
//! the scaffold (AST matching from generated Rust). Keys are derivable from
//! both directions.
//!
//! Examples:
//! - `contexts/money_transfer/functionality/transfer`
//! - `contexts/money_transfer/role/source/withdraw`
//! - `contexts/money_transfer/role/source/type`
//! - `data/account/operation/deposit`
//! - `data/account/type`
//! - `projections/balance/query/current_balance`

use std::fmt;
use std::str::FromStr;

/// The kind of trackable artefact within the build tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtefactEntryKind {
    Functionality,
    RoleMethod,
    DataOperation,
    ProjectionQuery,
    RoleType,
    DataType,
}

impl ArtefactEntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Functionality => "functionality",
            Self::RoleMethod => "role_method",
            Self::DataOperation => "data_operation",
            Self::ProjectionQuery => "projection_query",
            Self::RoleType => "role_type",
            Self::DataType => "data_type",
        }
    }
}

/// A canonical key identifying a single trackable artefact.
///
/// Display format matches the key strings stored in `.reen/build.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArtefactKey {
    /// `contexts/<context>/functionality/<name>`
    Functionality {
        context: String,
        name: String,
    },
    /// `contexts/<context>/role/<role>/<method>`
    RoleMethod {
        context: String,
        role: String,
        method: String,
    },
    /// `contexts/<context>/role/<role>/type`
    RoleType {
        context: String,
        role: String,
    },
    /// `data/<data>/operation/<name>`
    DataOperation {
        data: String,
        name: String,
    },
    /// `data/<data>/type`
    DataType {
        data: String,
    },
    /// `projections/<projection>/query/<name>`
    ProjectionQuery {
        projection: String,
        name: String,
    },
}

impl ArtefactKey {
    pub fn kind(&self) -> ArtefactEntryKind {
        match self {
            Self::Functionality { .. } => ArtefactEntryKind::Functionality,
            Self::RoleMethod { .. } => ArtefactEntryKind::RoleMethod,
            Self::RoleType { .. } => ArtefactEntryKind::RoleType,
            Self::DataOperation { .. } => ArtefactEntryKind::DataOperation,
            Self::DataType { .. } => ArtefactEntryKind::DataType,
            Self::ProjectionQuery { .. } => ArtefactEntryKind::ProjectionQuery,
        }
    }

    // ── Constructors from prepared YAML ──────────────────────────

    pub fn functionality(context: &str, name: &str) -> Self {
        Self::Functionality {
            context: crate::codegen::to_snake_case(context),
            name: crate::codegen::to_snake_case(name),
        }
    }

    pub fn role_method(context: &str, role: &str, method: &str) -> Self {
        Self::RoleMethod {
            context: crate::codegen::to_snake_case(context),
            role: crate::codegen::to_snake_case(role),
            method: crate::codegen::to_snake_case(method),
        }
    }

    pub fn role_type(context: &str, role: &str) -> Self {
        Self::RoleType {
            context: crate::codegen::to_snake_case(context),
            role: crate::codegen::to_snake_case(role),
        }
    }

    pub fn data_operation(data: &str, name: &str) -> Self {
        Self::DataOperation {
            data: crate::codegen::to_snake_case(data),
            name: crate::codegen::to_snake_case(name),
        }
    }

    pub fn data_type(data: &str) -> Self {
        Self::DataType {
            data: crate::codegen::to_snake_case(data),
        }
    }

    pub fn projection_query(projection: &str, name: &str) -> Self {
        Self::ProjectionQuery {
            projection: crate::codegen::to_snake_case(projection),
            name: crate::codegen::to_snake_case(name),
        }
    }

    // ── Constructors from Rust AST ───────────────────────────────

    /// Derive a key from a Rust method in an impl block, given the file path
    /// and enclosing type name.
    ///
    /// `file_subdir` is one of "contexts", "data", "projections".
    /// `type_name` is the snake_case filename stem (e.g. "money_transfer").
    /// `method_name` is the method identifier.
    /// `is_public` indicates whether the method is `pub`.
    pub fn from_rust_method(
        file_subdir: &str,
        type_name: &str,
        method_name: &str,
        is_public: bool,
    ) -> Option<Self> {
        let type_snake = crate::codegen::to_snake_case(type_name);
        let method_snake = crate::codegen::to_snake_case(method_name);

        match file_subdir {
            "contexts" => {
                if is_public {
                    Some(Self::Functionality {
                        context: type_snake,
                        name: method_snake,
                    })
                } else {
                    // Private method — try to parse as role_method: <role>_<method>
                    // We can't know the role from just the method name without the
                    // prepared YAML, so we store the canonical name directly.
                    // The caller must resolve this against the prepared artifact.
                    None
                }
            }
            "data" => {
                if is_public {
                    Some(Self::DataOperation {
                        data: type_snake,
                        name: method_snake,
                    })
                } else {
                    None
                }
            }
            "projections" => {
                if is_public {
                    Some(Self::ProjectionQuery {
                        projection: type_snake,
                        name: method_snake,
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Derive a key for a private context method using the known role method
    /// canonical name from the prepared YAML.
    pub fn from_rust_role_method(
        context: &str,
        role: &str,
        method: &str,
    ) -> Self {
        Self::RoleMethod {
            context: crate::codegen::to_snake_case(context),
            role: crate::codegen::to_snake_case(role),
            method: crate::codegen::to_snake_case(method),
        }
    }
}

impl fmt::Display for ArtefactKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Functionality { context, name } => {
                write!(f, "contexts/{}/functionality/{}", context, name)
            }
            Self::RoleMethod { context, role, method } => {
                write!(f, "contexts/{}/role/{}/{}", context, role, method)
            }
            Self::RoleType { context, role } => {
                write!(f, "contexts/{}/role/{}/type", context, role)
            }
            Self::DataOperation { data, name } => {
                write!(f, "data/{}/operation/{}", data, name)
            }
            Self::DataType { data } => {
                write!(f, "data/{}/type", data)
            }
            Self::ProjectionQuery { projection, name } => {
                write!(f, "projections/{}/query/{}", projection, name)
            }
        }
    }
}

impl FromStr for ArtefactKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('/').collect();
        match parts.as_slice() {
            ["contexts", ctx, "functionality", name] => Ok(Self::Functionality {
                context: ctx.to_string(),
                name: name.to_string(),
            }),
            ["contexts", ctx, "role", role, "type"] => Ok(Self::RoleType {
                context: ctx.to_string(),
                role: role.to_string(),
            }),
            ["contexts", ctx, "role", role, method] => Ok(Self::RoleMethod {
                context: ctx.to_string(),
                role: role.to_string(),
                method: method.to_string(),
            }),
            ["data", data, "operation", name] => Ok(Self::DataOperation {
                data: data.to_string(),
                name: name.to_string(),
            }),
            ["data", data, "type"] => Ok(Self::DataType {
                data: data.to_string(),
            }),
            ["projections", proj, "query", name] => Ok(Self::ProjectionQuery {
                projection: proj.to_string(),
                name: name.to_string(),
            }),
            _ => Err(format!("invalid artefact key: {}", s)),
        }
    }
}

// Serde: serialize as the display string
impl serde::Serialize for ArtefactKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for ArtefactKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_parse_round_trip() {
        let keys = vec![
            ArtefactKey::functionality("MoneyTransfer", "transfer"),
            ArtefactKey::role_method("MoneyTransfer", "source", "withdraw"),
            ArtefactKey::role_type("MoneyTransfer", "source"),
            ArtefactKey::data_operation("Account", "deposit"),
            ArtefactKey::data_type("Account"),
            ArtefactKey::projection_query("AccountBalance", "current_balance"),
        ];

        for key in &keys {
            let s = key.to_string();
            let parsed: ArtefactKey = s.parse().unwrap();
            assert_eq!(&parsed, key, "round-trip failed for {}", s);
        }
    }

    #[test]
    fn display_format() {
        assert_eq!(
            ArtefactKey::functionality("MoneyTransfer", "transfer").to_string(),
            "contexts/money_transfer/functionality/transfer"
        );
        assert_eq!(
            ArtefactKey::role_method("MoneyTransfer", "source", "withdraw").to_string(),
            "contexts/money_transfer/role/source/withdraw"
        );
        assert_eq!(
            ArtefactKey::role_type("MoneyTransfer", "source").to_string(),
            "contexts/money_transfer/role/source/type"
        );
        assert_eq!(
            ArtefactKey::data_operation("Account", "deposit").to_string(),
            "data/account/operation/deposit"
        );
        assert_eq!(
            ArtefactKey::data_type("Account").to_string(),
            "data/account/type"
        );
        assert_eq!(
            ArtefactKey::projection_query("AccountBalance", "current_balance").to_string(),
            "projections/account_balance/query/current_balance"
        );
    }

    #[test]
    fn parse_invalid_key() {
        assert!("invalid/key".parse::<ArtefactKey>().is_err());
        assert!("".parse::<ArtefactKey>().is_err());
        assert!("contexts/foo".parse::<ArtefactKey>().is_err());
    }

    #[test]
    fn serde_json_round_trip() {
        let key = ArtefactKey::functionality("MoneyTransfer", "transfer");
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"contexts/money_transfer/functionality/transfer\"");
        let parsed: ArtefactKey = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn kind_classification() {
        assert_eq!(
            ArtefactKey::functionality("X", "y").kind(),
            ArtefactEntryKind::Functionality
        );
        assert_eq!(
            ArtefactKey::role_method("X", "r", "m").kind(),
            ArtefactEntryKind::RoleMethod
        );
        assert_eq!(
            ArtefactKey::role_type("X", "r").kind(),
            ArtefactEntryKind::RoleType
        );
        assert_eq!(
            ArtefactKey::data_operation("X", "y").kind(),
            ArtefactEntryKind::DataOperation
        );
        assert_eq!(
            ArtefactKey::data_type("X").kind(),
            ArtefactEntryKind::DataType
        );
        assert_eq!(
            ArtefactKey::projection_query("X", "y").kind(),
            ArtefactEntryKind::ProjectionQuery
        );
    }

    #[test]
    fn from_rust_method_contexts() {
        let key = ArtefactKey::from_rust_method("contexts", "money_transfer", "transfer", true);
        assert_eq!(key, Some(ArtefactKey::functionality("money_transfer", "transfer")));

        // Private methods in contexts can't be resolved without prepared YAML
        let key = ArtefactKey::from_rust_method("contexts", "money_transfer", "source_withdraw", false);
        assert_eq!(key, None);
    }

    #[test]
    fn from_rust_method_data() {
        let key = ArtefactKey::from_rust_method("data", "account", "deposit", true);
        assert_eq!(key, Some(ArtefactKey::data_operation("account", "deposit")));

        let key = ArtefactKey::from_rust_method("data", "account", "helper", false);
        assert_eq!(key, None);
    }

    #[test]
    fn from_rust_method_projections() {
        let key = ArtefactKey::from_rust_method("projections", "account_balance", "current_balance", true);
        assert_eq!(key, Some(ArtefactKey::projection_query("account_balance", "current_balance")));
    }
}
