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
use std::sync::Arc;

pub struct DBTTemplater;

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
            let files = [
                (
                    "sqruff_templaters/__init__.py",
                    include_str!("sqruff_templaters/__init__.py"),
                ),
                (
                    "sqruff_templaters/dbt_templater.py",
                    include_str!("sqruff_templaters/dbt_templater.py"),
                ),
                (
                    "sqruff_templaters/jinja_templater.py",
                    include_str!("sqruff_templaters/jinja_templater.py"),
                ),
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

            let main_module = PyModule::import(py, "sqruff_templaters.dbt_templater")?;
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
