use sqruff_lib_core::errors::SQLFluffUserError;
use sqruff_lib_core::templaters::base::TemplatedFile;

use crate::cli::formatters::OutputStreamFormatter;
use crate::core::config::FluffConfig;
use crate::templaters::placeholder::PlaceholderTemplater;
use crate::templaters::raw::RawTemplater;

#[cfg(feature = "python")]
use crate::templaters::python::PythonTemplater;

pub mod placeholder;
#[cfg(feature = "python")]
pub mod python;
pub mod raw;

pub static RAW_TEMPLATER: RawTemplater = RawTemplater;
pub static PLACEHOLDER_TEMPLATER: PlaceholderTemplater = PlaceholderTemplater;
#[cfg(feature = "python")]
pub static PYTHON_TEMPLATER: PythonTemplater = PythonTemplater;

// templaters returns all the templaters that are available in the library
#[cfg(feature = "python")]
pub static TEMPLATERS: [&'static dyn Templater; 3] =
    [&RAW_TEMPLATER, &PLACEHOLDER_TEMPLATER, &PYTHON_TEMPLATER];

#[cfg(not(feature = "python"))]
pub static TEMPLATERS: [&'static dyn Templater; 2] = [&RAW_TEMPLATER, &PLACEHOLDER_TEMPLATER];

pub trait Templater: Send + Sync {
    /// The name of the templater.
    fn name(&self) -> &'static str;

    /// Description of the templater.
    fn description(&self) -> &'static str;

    /// Process a string and return a TemplatedFile.
    fn process(
        &self,
        in_str: &str,
        f_name: &str,
        config: Option<&FluffConfig>,
        formatter: Option<&OutputStreamFormatter>,
    ) -> Result<TemplatedFile, SQLFluffUserError>;
}
