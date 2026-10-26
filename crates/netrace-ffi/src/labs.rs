//! Labs: the bundled catalogue, a project's lab, and checking the work.

use netrace_labs as labs;

use crate::{Project, Simulation};

/// A bundled lab, for the lab browser.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LabSummary {
    pub id: String,
    pub title: String,
    pub topic: String,
    /// The brief's first paragraph.
    pub summary: String,
    pub task_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LabTaskInfo {
    pub title: String,
    /// Markdown.
    pub detail: String,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LabInfo {
    pub title: String,
    pub topic: String,
    /// Markdown.
    pub brief: String,
    pub tasks: Vec<LabTaskInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LabCheckResult {
    pub passed: bool,
    pub label: String,
    pub feedback: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LabTaskResult {
    pub done: bool,
    pub checks: Vec<LabCheckResult>,
}

/// Every lab that ships with netrace, in teaching order.
#[uniffi::export]
pub fn bundled_labs() -> Vec<LabSummary> {
    labs::bundled_labs()
        .into_iter()
        .map(|b| {
            let lab = b.lab();
            LabSummary {
                id: b.id.into(),
                summary: lab.brief.split("\n\n").next().unwrap_or("").to_string(),
                task_count: lab.tasks.len() as u32,
                title: lab.title,
                topic: lab.topic,
            }
        })
        .collect()
}

/// A bundled lab's starting project, as file contents.
#[uniffi::export]
pub fn bundled_lab_json(id: String) -> Option<Vec<u8>> {
    labs::bundled_labs()
        .into_iter()
        .find(|b| b.id == id)
        .map(|b| b.json().to_vec())
}

#[uniffi::export]
impl Project {
    /// The project's lab, if it is one.
    pub fn lab(&self) -> Option<LabInfo> {
        let project = self.lock();
        let lab = project.lab()?;
        Some(LabInfo {
            title: lab.title.clone(),
            topic: lab.topic.clone(),
            brief: lab.brief.clone(),
            tasks: lab
                .tasks
                .iter()
                .map(|t| LabTaskInfo {
                    title: t.title.clone(),
                    detail: t.detail.clone(),
                    hint: t.hint.clone(),
                })
                .collect(),
        })
    }
}

#[uniffi::export]
impl Simulation {
    /// Checks the lab against the topology as it is now (runs its own pings
    /// in a separate simulation, so the user's trace is untouched).
    pub fn lab_results(&self) -> Vec<LabTaskResult> {
        let project = self.lock().project().clone();
        labs::evaluate(&project)
            .into_iter()
            .map(|t| LabTaskResult {
                done: t.done,
                checks: t
                    .checks
                    .into_iter()
                    .map(|c| LabCheckResult {
                        passed: c.passed,
                        label: c.label,
                        feedback: c.feedback,
                    })
                    .collect(),
            })
            .collect()
    }
}

/// The showcase network the intro tour walks through, as file contents.
#[uniffi::export]
pub fn tour_project_json() -> Vec<u8> {
    labs::tour_json().to_vec()
}
