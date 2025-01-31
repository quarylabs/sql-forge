use super::python::PythonTemplatedFile;
use super::Templater;
use crate::core::config::FluffConfig;
use crate::templaters::python_shared::PythonFluffConfig;
use crate::templaters::Formatter;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyMapping, PyString};
use pyo3::{Py, PyAny, Python};
use sqruff_lib_core::errors::SQLFluffUserError;
use sqruff_lib_core::templaters::base::TemplatedFile;
use std::ffi::CString;
use std::sync::Arc;

pub struct DBTTemplater;

const DBT_FILE: &str = include_str!("sqruff_templaters/dbt_templater.py");

impl Templater for DBTTemplater {
    fn name(&self) -> &'static str {
        "dbt"
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
                    "jinja_templater",
                    include_str!("sqruff_templaters/jinja_templater.py"),
                ),
                (
                    "jinja_templater_builtins_common",
                    include_str!("sqruff_templaters/jinja_templater_builtins_common.py"),
                ),
                (
                    "jinja_templater_builtins_dbt",
                    include_str!("sqruff_templaters/jinja_templater_builtins_dbt.py"),
                ),
                (
                    "jinja_templater_tracers",
                    include_str!("sqruff_templaters/jinja_templater_tracers.py"),
                ),
                (
                    "python_templater",
                    include_str!("sqruff_templaters/python_templater.py"),
                ),
            ];

            // Add virtual environment site-packages to sys.path
            let os = py.import("os").unwrap();
            let environ = os.getattr("environ").unwrap();
            let environ = environ.downcast::<PyMapping>().unwrap();
            if let Ok(environ) = environ.get_item("VIRTUAL_ENV") {
                let virtual_env = environ.downcast::<PyString>();
                if let Ok(virtual_env) = virtual_env {
                    let virtual_env = virtual_env.to_string();
                    // figure out which python folder sits in virtual_env
                    let virtual_env_lib = format!("{}/lib", virtual_env);
                    // look at the contents of the lib folder
                    let lib_folder = std::fs::read_dir(virtual_env_lib).unwrap();
                    for entry in lib_folder {
                        let entry = entry.unwrap();
                        let entry_path = entry.path();
                        if entry_path.is_dir() {
                            let entry_path_str = entry_path.to_str().unwrap();
                            if entry_path_str.contains("python") {
                                let site_packages = entry_path.join("site-packages");
                                path.call_method1("append", (site_packages.to_str().unwrap(),))?;
                            }
                        }
                    }
                }
            };

            // Create temp folder
            let temp_folder = std::env::temp_dir();
            let temp_folder_templaters = temp_folder.join("sqruff_templaters");
            std::fs::create_dir_all(&temp_folder_templaters).unwrap();
            for (name, file_contents) in files.iter() {
                let file_name = temp_folder_templaters.join(format!("{}.py", name));
                std::fs::write(file_name, file_contents).unwrap();
            }

            let syspath = py
                .import("sys")?
                .getattr("path")?
                .downcast_into::<PyList>()?;
            syspath.insert(0, temp_folder)?;

            let file_contents = CString::new(DBT_FILE).unwrap();
            let main_module =
                PyModule::from_code(py, &file_contents, c"dbt_templater.py", c"dbt_templater")?;
            let fun: Py<PyAny> = main_module.getattr("process_from_rust")?.into();

            let py_dict = config.to_python_context(py, "dbt").unwrap();
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
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_dbt_simple() {
        let templater = DBTTemplater;

        let crate_location = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let file = crate_location.join("src").join("templaters").join("dbt.rs");
        let project_dir = file
            .parent()
            .unwrap()
            .join("sqruff_templaters")
            .join("sample_dbt")
            .canonicalize()
            .unwrap();
        let profiles_dir = project_dir.join(".profiles").canonicalize().unwrap();
        let file_path = project_dir
            .join("models/example/my_first_dbt_model.sql")
            .canonicalize()
            .unwrap();

        let config = format!(
            r#"
[sqruff]
templater = dbt

[sqruff:templater:dbt]
project_dir = {project_dir}
profiles_dir = {profiles_dir}
            "#,
            project_dir = project_dir.to_str().unwrap(),
            profiles_dir = profiles_dir.to_str().unwrap()
        );

        let fluff_config = FluffConfig::from_source(&config, None);

        let templated_file = templater
            .process(
                r#"
        {{ config(materialized='table') }}

        with source_data as (
            select 1 as id
            union all
            select null as id
        )
                "#,
                file_path.to_str().unwrap(),
                &fluff_config,
                &None,
            )
            .unwrap();

        assert!(!templated_file.sliced_file.is_empty());
    }
}
