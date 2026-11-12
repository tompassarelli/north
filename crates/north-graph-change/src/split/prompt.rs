use super::plan::Plan;

pub const SYSTEM: &str = "You plan how one code change splits into parts that separate workers build at the same time. Answer with one JSON object and nothing else.";

pub fn first(task: &str, map: &str) -> String {
    format!(
        "Task:\n{task}\n\nAnswer with JSON of this shape:\n{}\n\nRules:\n- Name every file the task will change or create, each in exactly one part, and mark a file the task creates with \"new\": true. Use only paths from the map below, apart from new files.\n- adds: symbols the file will define that do not exist yet. changes: existing symbols whose signature or behaviour other files rely on will change. uses: symbols from other files the new code will call. Write a method or a common name as Owner::name.\n- shared: the new types, traits and function signatures that more than one part needs. They land first with stub bodies so every part can start at once; keep them to signatures and types, each declared in a file some part fills.\n- Make as many parts as the work allows while no two parts edit one file; each part is a coherent piece with its own tests.\n\nMap of the repository at the base commit:\n{map}",
        Plan::shape()
    )
}

pub fn again(asked: &str, answer: &str, refusal: &str) -> String {
    format!("{asked}\n\nYour previous answer:\n{answer}\n\nThe split refused it against the base commit: {refusal}\nAnswer again with the whole corrected JSON object.")
}
