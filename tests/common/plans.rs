use bevy::prelude::Entity;
use proptest::prelude::*;
use proptest_derive::Arbitrary;

use super::runtime::Harness;

pub fn prompt_text() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z]{1,16}").expect("valid prompt strategy")
}

pub fn whitespace_text() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[ \\t\\n]{1,12}").expect("valid whitespace strategy")
}

#[derive(Debug, Clone, Arbitrary)]
pub struct ConversationPlan {
    #[proptest(strategy = "proptest::collection::vec(prompt_text(), 1..=6)")]
    pub prompts: Vec<String>,
    #[proptest(strategy = "proptest::collection::vec(any::<bool>(), 0..=6)")]
    pub branch_to_root: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationStep {
    pub turn: Entity,
    pub parent: Option<Entity>,
    pub text: String,
}

pub fn apply_conversation_plan(
    harness: &mut Harness,
    plan: &ConversationPlan,
) -> Vec<ConversationStep> {
    apply_plan(harness, plan, Harness::complete_with_fake_results)
}

pub fn apply_conversation_plan_without_execution(
    harness: &mut Harness,
    plan: &ConversationPlan,
) -> Vec<ConversationStep> {
    apply_plan(harness, plan, Harness::mark_completed)
}

fn apply_plan(
    harness: &mut Harness,
    plan: &ConversationPlan,
    mut complete: impl FnMut(&mut Harness, Entity),
) -> Vec<ConversationStep> {
    let mut root = None;
    let mut head = None;
    let mut steps = Vec::with_capacity(plan.prompts.len());

    for (index, text) in plan.prompts.iter().enumerate() {
        if index > 0 && plan.branch_to_root.get(index - 1).copied().unwrap_or(false) {
            harness.select_branch(root.expect("the first prompt establishes the root"));
            head = root;
        }
        let parent = head;
        let turn = harness.submit(text.clone());
        complete(harness, turn);
        root.get_or_insert(turn);
        head = Some(turn);
        steps.push(ConversationStep {
            turn,
            parent,
            text: text.clone(),
        });
    }

    steps
}
