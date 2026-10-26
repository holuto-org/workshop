//! Offline contract checks for the loopback Holuto fake, never the live API.

use std::{collections::BTreeMap, sync::LazyLock};

use jsonschema::{Validator, draft202012};
use serde_json::Value;
use uuid::Uuid;

static CONTRACT: LazyLock<Result<Contract, String>> = LazyLock::new(Contract::load);

struct Contract {
    operations: Vec<Operation>,
}

struct Operation {
    method: String,
    path: String,
    request: Option<Validator>,
    responses: BTreeMap<String, Option<Validator>>,
}

impl Contract {
    fn load() -> Result<Self, String> {
        let document: Value = serde_json::from_str(include_str!("../../../../openapi/agent.json"))
            .map_err(|error| error.to_string())?;
        let mut operations = Vec::new();
        for (path, item) in document
            .get("paths")
            .and_then(Value::as_object)
            .ok_or("missing paths")?
        {
            if !path.starts_with("/agent/") {
                continue;
            }
            for (method, operation) in item.as_object().ok_or("invalid path item")? {
                if !matches!(method.as_str(), "get" | "post") {
                    continue;
                }
                let method = method.to_ascii_uppercase();
                let request = Self::validator(
                    &document,
                    operation.pointer("/requestBody/content/application~1json/schema"),
                )?;
                let mut responses = BTreeMap::new();
                for (status, response) in operation
                    .get("responses")
                    .and_then(Value::as_object)
                    .ok_or("missing responses")?
                {
                    let response = if let Some(reference) =
                        response.get("$ref").and_then(Value::as_str)
                    {
                        document
                            .pointer(reference.strip_prefix('#').ok_or("external response ref")?)
                            .ok_or("missing response ref")?
                    } else {
                        response
                    };
                    let schema = response
                        .pointer("/content/application~1json/schema")
                        .or_else(|| response.pointer("/content/application~1problem+json/schema"));
                    let _old =
                        responses.insert(status.clone(), Self::validator(&document, schema)?);
                }
                operations.push(Operation {
                    method,
                    path: path.clone(),
                    request,
                    responses,
                });
            }
        }
        Ok(Self { operations })
    }

    fn validator(document: &Value, schema: Option<&Value>) -> Result<Option<Validator>, String> {
        schema
            .map(|schema| {
                let mut schema = schema.clone();
                let _old = schema
                    .as_object_mut()
                    .ok_or("schema is not an object")?
                    .insert(
                        "components".into(),
                        document
                            .get("components")
                            .ok_or("missing components")?
                            .clone(),
                    );
                draft202012::options()
                    .should_validate_formats(true)
                    .build(&schema)
                    .map_err(|error| error.to_string())
            })
            .transpose()
    }

    fn validate(&self, method: &str, path: &str, status: &str, body: &Value) -> Result<(), String> {
        let path = path.split('?').next().ok_or("missing path")?;
        let operation = self
            .operations
            .iter()
            .find(|operation| operation.method == method && matches_path(&operation.path, path))
            .ok_or_else(|| format!("operation absent from OpenAPI: {method} {path}"))?;
        let validator = if status == "request" {
            &operation.request
        } else {
            operation
                .responses
                .get(status)
                .or_else(|| operation.responses.get("default"))
                .ok_or_else(|| format!("response absent from OpenAPI: {method} {path} {status}"))?
        };
        match validator {
            Some(validator) => validator.validate(body).map_err(|error| {
                format!("{method} {} {status}: {}", operation.path, error.masked())
            }),
            None if body.is_null() => Ok(()),
            None => Err(format!(
                "unexpected body: {method} {} {status}",
                operation.path
            )),
        }
    }
}

fn matches_path(template: &str, path: &str) -> bool {
    let mut expected = template.split('/');
    let mut actual = path.split('/');
    loop {
        match (expected.next(), actual.next()) {
            (None, None) => return true,
            (Some("{jobId}"), Some(value)) if Uuid::parse_str(value).is_ok() => {},
            (Some(expected), Some(actual)) if expected == actual => {},
            _ => return false,
        }
    }
}

pub(crate) fn validate_request(method: &str, path: &str, body: &Value) -> Result<(), String> {
    CONTRACT
        .as_ref()
        .map_err(Clone::clone)?
        .validate(method, path, "request", body)
}

pub(crate) fn validate_response(
    method: &str,
    path: &str,
    status: u16,
    body: &Value,
) -> Result<(), String> {
    CONTRACT
        .as_ref()
        .map_err(Clone::clone)?
        .validate(method, path, &status.to_string(), body)
}

#[cfg(test)]
mod tests;
