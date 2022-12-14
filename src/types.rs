#[cfg(feature = "pyo3")]
use pyo3::{exceptions::PyValueError, pyclass, pymethods, PyResult};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use tracing::warn;

/// One of `~=` `==` `!=` `<=` `>=` `<` `>` `===`
#[derive(Eq, PartialEq, Debug, Hash, Clone)]
#[cfg_attr(feature = "pyo3", pyclass)]
pub enum Operator {
    /// `== 1.2.3`
    Equal,
    /// `== 1.2.*`
    EqualStar,
    /// `===` (discouraged)
    ///
    /// <https://peps.python.org/pep-0440/#arbitrary-equality>
    ///
    /// "Use of this operator is heavily discouraged and tooling MAY display a warning when it is used"
    // clippy doesn't like this: #[deprecated = "Use of this operator is heavily discouraged"]
    ExactEqual,
    /// `!= 1.2.3`
    NotEqual,
    /// `!= 1.2.*`
    NotEqualStar,
    /// `~=`
    TildeEqual,
    /// `<`
    LessThan,
    /// `<=`
    LessThanEqual,
    /// `>`
    GreaterThan,
    /// `>=`
    GreaterThanEqual,
}

impl FromStr for Operator {
    type Err = String;

    /// Notably, this does not know about star versions, it just assumes the base operator
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let operator = match s {
            "==" => Self::Equal,
            "===" => {
                warn!("Using arbitrary equality (`===`) is discouraged");
                #[allow(deprecated)]
                Self::ExactEqual
            }
            "!=" => Self::NotEqual,
            "~=" => Self::TildeEqual,
            "<" => Self::LessThan,
            "<=" => Self::LessThanEqual,
            ">" => Self::GreaterThan,
            ">=" => Self::GreaterThanEqual,
            // Should be forbidden by the regex if called from normal parsing
            other => {
                return Err(format!(
                    "No such comparison operator '{}', must be one of ~= == != <= >= < > ===",
                    other
                ))
            }
        };
        Ok(operator)
    }
}

impl Display for Operator {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let operator = match self {
            Operator::Equal => "==",
            Operator::EqualStar => "==",
            #[allow(deprecated)]
            Operator::ExactEqual => "===",
            Operator::NotEqual => "!=",
            Operator::NotEqualStar => "!=",
            Operator::TildeEqual => "~=",
            Operator::LessThan => "<",
            Operator::LessThanEqual => "<=",
            Operator::GreaterThan => ">",
            Operator::GreaterThanEqual => ">=",
        };

        write!(f, "{}", operator)
    }
}

/// A version range such such as `>1.2.3`, `<=4!5.6.7-a8.post9.dev0` or `== 4.1.*`. Parse with
/// [VersionSpecifier::from_str]
///
/// ```rust
/// use std::str::FromStr;
/// use pep440_rs::{Version, VersionSpecifier};
///
/// let version = Version::from_str("1.19").unwrap();
/// let version_specifier = VersionSpecifier::from_str("== 1.*").unwrap();
/// assert!(version_specifier.contains(&version));
/// ```
#[cfg_attr(feature = "pyo3", pyclass)]
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct VersionSpecifier {
    /// ~=|==|!=|<=|>=|<|>|===, plus whether the version ended with a star
    pub(crate) operator: Operator,
    /// The whole version part behind the operator
    pub(crate) version: Version,
}

#[cfg(feature = "pyo3")]
#[pymethods]
impl VersionSpecifier {
    // Since we don't bring FromStr to python
    #[new]
    #[doc(hidden)]
    pub fn parse(version_specifier: String) -> PyResult<Self> {
        Self::from_str(&version_specifier).map_err(PyValueError::new_err)
    }

    #[doc(hidden)]
    pub fn __contains__(&self, version: &Version) -> bool {
        self.contains(version)
    }
}

impl VersionSpecifier {
    /// Build from parts, validating that the operator is allowed with that version
    pub fn new(operator: Operator, version: Version) -> Result<Self, String> {
        // "Local version identifiers are NOT permitted in this version specifier."
        if let Some(local) = &version.local {
            if matches!(
                operator,
                Operator::GreaterThan
                    | Operator::GreaterThanEqual
                    | Operator::LessThan
                    | Operator::LessThanEqual
                    | Operator::TildeEqual
                    | Operator::EqualStar
                    | Operator::NotEqualStar
            ) {
                return Err(format!(
                    "You can't mix a {} operator with a local version (`+{}`)",
                    operator,
                    local
                        .iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<String>>()
                        .join(".")
                ));
            }
        }

        if operator == Operator::TildeEqual && version.release.len() < 2 {
            return Err(
                "The ~= operator requires at least two parts in the release version".to_string(),
            );
        }

        Ok(Self { operator, version })
    }

    /// Get the operator, e.g. `>=` in `>= 2.0.0`
    pub fn operator(&self) -> &Operator {
        &self.operator
    }

    /// Get the version, e.g. `<=` in `<= 2.0.0`
    pub fn version(&self) -> &Version {
        &self.version
    }
}

/// A version number such as `1.2.3` or `4!5.6.7-a8.post9.dev0`.
///
/// Beware that the sorting implemented with [Ord] and [Eq] is not consistent with the operators
/// from PEP 440, i.e. compare two versions in rust with `>` gives a different result than a
/// VersionSpecifier with `>` as operator.
///
/// Parse with [Version::from_str]:
///
/// ```rust
/// use std::str::FromStr;
/// use pep440_rs::Version;
///
/// let version = Version::from_str("1.19").unwrap();
/// ```
#[cfg_attr(feature = "pyo3", pyclass)]
#[derive(Debug, Clone)]
pub struct Version {
    /// The [versioning epoch](https://peps.python.org/pep-0440/#version-epochs). Normally just 0,
    /// but you can increment it if you switched the versioning scheme.
    #[cfg_attr(feature = "pyo3", pyo3(get, set))]
    pub epoch: usize,
    /// The normal number part of the version
    /// (["final release"](https://peps.python.org/pep-0440/#final-releases)),
    /// such a `1.2.3` in `4!1.2.3-a8.post9.dev1`
    ///
    /// Note that we drop the * placeholder by moving it to `Operator`
    #[cfg_attr(feature = "pyo3", pyo3(get, set))]
    pub release: Vec<usize>,
    /// The [prerelease](https://peps.python.org/pep-0440/#pre-releases), i.e. alpha, beta or rc
    /// plus a number
    ///
    /// Note that whether this is Some influences the version
    /// range matching since normally we exclude all prerelease versions
    #[cfg_attr(feature = "pyo3", pyo3(get, set))]
    pub pre: Option<(PreRelease, usize)>,
    /// The [Post release version](https://peps.python.org/pep-0440/#post-releases),
    /// higher post version are preferred over lower post or none-post versions
    #[cfg_attr(feature = "pyo3", pyo3(get, set))]
    pub post: Option<usize>,
    /// The [developmental release](https://peps.python.org/pep-0440/#developmental-releases),
    /// if any
    #[cfg_attr(feature = "pyo3", pyo3(get, set))]
    pub dev: Option<usize>,
    /// A [local version identifier](https://peps.python.org/pep-0440/#local-version-identifiers)
    /// such as `+deadbeef` in `1.2.3+deadbeef`
    ///
    /// > They consist of a normal public version identifier (as defined in the previous section),
    /// > along with an arbitrary “local version label”, separated from the public version
    /// > identifier by a plus. Local version labels have no specific semantics assigned, but some
    /// > syntactic restrictions are imposed.
    pub local: Option<Vec<LocalSegment>>,
}

#[cfg_attr(feature = "pyo3", pymethods)]
impl Version {
    /// Parses a PEP 440 version string
    #[cfg(feature = "pyo3")]
    #[new]
    pub fn parse(version: String) -> PyResult<Self> {
        Self::from_str(&version).map_err(PyValueError::new_err)
    }

    /// Whether this is an alpha/beta/rc or dev version
    pub fn any_prerelease(&self) -> bool {
        self.is_pre() || self.is_dev()
    }

    /// Whether this is an alpha/beta/rc version
    pub fn is_pre(&self) -> bool {
        self.pre.is_some()
    }

    /// Whether this is a dev version
    pub fn is_dev(&self) -> bool {
        self.dev.is_some()
    }

    /// Whether this is a post version
    pub fn is_post(&self) -> bool {
        self.post.is_some()
    }

    /// Whether this is a local version (e.g. `1.2.3+localsuffixesareweird`)
    pub fn is_local(&self) -> bool {
        self.local.is_some()
    }
}
impl Version {
    /// For PEP 440 specifier matching: "Except where specifically noted below, local version
    /// identifiers MUST NOT be permitted in version specifiers, and local version labels MUST be
    /// ignored entirely when checking if candidate versions match a given version specifier."
    pub(crate) fn without_local(&self) -> Self {
        Self {
            local: None,
            ..self.clone()
        }
    }
}

/// Shows normalized version
impl Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let epoch = if self.epoch == 0 {
            "".to_string()
        } else {
            format!("{}!", self.epoch)
        };
        let release = self
            .release
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<String>>()
            .join(".");
        let pre = self
            .pre
            .as_ref()
            .map(|(pre_kind, pre_version)| format!("{}{}", pre_kind, pre_version))
            .unwrap_or_default();
        let post = self
            .post
            .map(|post| format!(".post{}", post))
            .unwrap_or_default();
        let dev = self
            .dev
            .map(|dev| format!(".dev{}", dev))
            .unwrap_or_default();
        let local = self
            .local
            .as_ref()
            .map(|segments| {
                format!(
                    "+{}",
                    segments
                        .iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<String>>()
                        .join(".")
                )
            })
            .unwrap_or_default();
        write!(f, "{}{}{}{}{}{}", epoch, release, pre, post, dev, local)
    }
}

/// Optional prerelease modifier (alpha, beta or release candidate) appended to version
///
/// <https://peps.python.org/pep-0440/#pre-releases>
#[derive(PartialEq, Eq, Debug, Hash, Clone, Ord, PartialOrd)]
#[cfg_attr(feature = "pyo3", pyclass)]
pub enum PreRelease {
    /// alpha prerelease
    Alpha,
    /// beta prerelease
    Beta,
    /// release candidate prerelease
    Rc,
}

impl FromStr for PreRelease {
    type Err = String;

    fn from_str(prerelease: &str) -> Result<Self, Self::Err> {
        match prerelease.to_lowercase().as_str() {
            "a" | "alpha" => Ok(Self::Alpha),
            "b" | "beta" => Ok(Self::Beta),
            "c" | "rc" | "pre" | "preview" => Ok(Self::Rc),
            _ => Err(format!(
                "'{}' isn't recognized as alpha, beta or release candidate",
                prerelease
            )),
        }
    }
}

impl Display for PreRelease {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Alpha => write!(f, "a"),
            Self::Beta => write!(f, "b"),
            Self::Rc => write!(f, "rc"),
        }
    }
}

/// A part of the [local version identifier](<https://peps.python.org/pep-0440/#local-version-identifiers>)
///
/// Local versions are a mess:
///
/// > Comparison and ordering of local versions considers each segment of the local version
/// > (divided by a .) separately. If a segment consists entirely of ASCII digits then that section
/// > should be considered an integer for comparison purposes and if a segment contains any ASCII
/// > letters then that segment is compared lexicographically with case insensitivity. When
/// > comparing a numeric and lexicographic segment, the numeric section always compares as greater
/// > than the lexicographic segment. Additionally a local version with a great number of segments
/// > will always compare as greater than a local version with fewer segments, as long as the
/// > shorter local version’s segments match the beginning of the longer local version’s segments
/// > exactly.
///
/// Luckily the default Ord impl for Vec<LocalSegment> matches the PEP 440 rules
#[derive(Eq, PartialEq, Debug, Clone, Hash)]
pub enum LocalSegment {
    /// Not-parseable as integer segment of local version
    String(String),
    /// Inferred integer segment of local version
    Number(usize),
}

impl Display for LocalSegment {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(string) => write!(f, "{}", string),
            Self::Number(number) => write!(f, "{}", number),
        }
    }
}

impl PartialOrd for LocalSegment {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl FromStr for LocalSegment {
    /// This can be a never type when stabilized
    type Err = ();

    fn from_str(segment: &str) -> Result<Self, Self::Err> {
        Ok(if let Ok(number) = segment.parse::<usize>() {
            Self::Number(number)
        } else {
            // "and if a segment contains any ASCII letters then that segment is compared lexicographically with case insensitivity"
            Self::String(segment.to_lowercase())
        })
    }
}

/// Error with span information (unicode width) inside the parsed line
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Pep440Error {
    /// The actual error message
    pub message: String,
    /// The string that failed to parse
    pub line: String,
    /// First character for underlining (unicode width)
    pub start: usize,
    /// Number of characters to underline (unicode width)
    pub width: usize,
}

impl Display for Pep440Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Failed to parse version:")?;
        writeln!(f, "{}", self.line)?;
        writeln!(f, "{}{}", " ".repeat(self.start), "^".repeat(self.width))?;
        Ok(())
    }
}
