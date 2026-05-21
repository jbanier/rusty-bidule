use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowDefinition {
    pub workflow_type: String,
    pub max_followups: usize,
    pub steps: Vec<WorkflowStepDefinition>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct WorkflowStepDefinition {
    pub name: Option<String>,
    pub prompt: Option<String>,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default)]
    pub max_attempts: Option<usize>,
    #[serde(default)]
    pub local_tools: Option<Vec<String>>,
    #[serde(default)]
    pub mcp_servers: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct WorkflowYaml {
    #[serde(rename = "type")]
    workflow_type: String,
    #[serde(default)]
    max_followups: Option<usize>,
    #[serde(default)]
    steps: Vec<WorkflowStepDefinition>,
}

pub fn parse_workflow_definition(raw: &str) -> Option<WorkflowDefinition> {
    let yaml = serde_yaml::from_str::<WorkflowYaml>(raw).ok()?;
    match yaml.workflow_type.as_str() {
        "iterative_research" | "supervised_steps" => Some(WorkflowDefinition {
            workflow_type: yaml.workflow_type,
            max_followups: yaml.max_followups.unwrap_or(1).min(5),
            steps: yaml.steps,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_workflow_definition;

    #[test]
    fn parses_supported_supervised_workflow() {
        let workflow = parse_workflow_definition(
            r#"
type: supervised_steps
steps:
  - name: collect
    prompt: Collect evidence
    approval_required: true
    local_tools:
      - local__time
"#,
        )
        .unwrap();

        assert_eq!(workflow.workflow_type, "supervised_steps");
        assert_eq!(workflow.steps.len(), 1);
        assert!(workflow.steps[0].approval_required);
    }

    #[test]
    fn unsupported_or_plain_text_workflows_are_guidance_only() {
        assert!(parse_workflow_definition("Run these steps by hand.").is_none());
        assert!(parse_workflow_definition("type: branching").is_none());
    }
}
