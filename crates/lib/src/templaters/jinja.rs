use super::python::PythonTemplatedFile;
use super::Templater;
use crate::core::config::FluffConfig;
use crate::templaters::python_shared::add_temp_files_to_site_packages;
use crate::templaters::python_shared::add_venv_site_packages;
use crate::templaters::python_shared::PythonFluffConfig;
use crate::templaters::Formatter;
use pyo3::prelude::*;
use pyo3::{Py, PyAny, Python};
use sqruff_lib_core::errors::SQLFluffUserError;
use sqruff_lib_core::templaters::base::TemplatedFile;
use std::ffi::CString;
use std::sync::Arc;

pub struct JinjaTemplater;

const JINJA_FILE: &str = include_str!("sqruff_templaters/jinja_templater.py");

impl Templater for JinjaTemplater {
    fn name(&self) -> &'static str {
        "jinja"
    }

    fn description(&self) -> &'static str {
        "Not fully implemented yet. More details to come."
    }

    fn process(
        &self,
        in_str: &str,
        f_name: &str,
        config: &FluffConfig,
        _: &Option<Arc<dyn Formatter>>,
    ) -> Result<TemplatedFile, SQLFluffUserError> {
        let templated_file = Python::with_gil(|py| -> PyResult<TemplatedFile> {
            let sysconfig = py.import("sysconfig")?;
            let paths = sysconfig.call_method0("get_paths")?;
            let site_packages = paths.get_item("purelib")?;
            let site_packages: &str = site_packages.extract()?;

            let sys = py.import("sys")?;
            let path = sys.getattr("path")?;
            path.call_method1("append", (site_packages,))?; // append site-packages

            let files = [
                (
                    "sqruff_templaters/jinja_templater_builtins_common.py",
                    include_str!("sqruff_templaters/jinja_templater_builtins_common.py"),
                ),
                (
                    "sqruff_templaters/jinja_templater_builtins_dbt.py",
                    include_str!("sqruff_templaters/jinja_templater_builtins_dbt.py"),
                ),
                (
                    "sqruff_templaters/jinja_templater_tracers.py",
                    include_str!("sqruff_templaters/jinja_templater_tracers.py"),
                ),
                (
                    "sqruff_templaters/python_templater.py",
                    include_str!("sqruff_templaters/python_templater.py"),
                ),
            ];

            add_venv_site_packages(py)?;
            add_temp_files_to_site_packages(py, &files)?;

            let file_contents = CString::new(JINJA_FILE).unwrap();
            let main_module = PyModule::from_code(py, &file_contents, c"", c"")?;
            let fun: Py<PyAny> = main_module.getattr("process_from_rust")?.into();

            let py_dict = config.to_python_context(py, "jinja").unwrap();
            let python_fluff_config: PythonFluffConfig = config.clone().into();
            let args = (
                in_str.to_string(),
                f_name.to_string(),
                python_fluff_config.to_json_string(),
                py_dict,
            );
            let returned = fun.call1(py, args);

            // Parse the returned value
            let returned = returned?;
            let templated_file: PythonTemplatedFile = returned.extract(py)?;
            Ok(templated_file.to_templated_file())
        })
        .map_err(|e| SQLFluffUserError::new(format!("Python templater error: {:?}", e)))?;
        Ok(templated_file)
    }
}

#[cfg(test)]
mod tests {
    use crate::core::config::FluffConfig;

    use super::*;

    const JINJA_STRING: &str = "
{% set event_columns = ['campaign', 'click_item'] %}

SELECT
    event_id
    {% for event_column in event_columns %}
    , {{ event_column }}
    {% endfor %}
FROM events
";

    #[test]
    fn test_jinja_templater() {
        let source = r"
    [sqruff]
    templater = jinja
        ";
        let config = FluffConfig::from_source(source, None);
        let templater = JinjaTemplater;

        let processed = templater
            .process(JINJA_STRING, "test.sql", &config, &None)
            .unwrap();

        assert_eq!(
            processed.templated(),
            "\n\n\nSELECT\n    event_id\n    \n    , campaign\n    \n    , click_item\n    \nFROM events\n"
        )
    }
}
