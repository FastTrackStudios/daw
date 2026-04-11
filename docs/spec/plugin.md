# Obsidian Plugin Spec

## WASM API

### t[plugin.wasm]
The `obsidian-plugin` crate compiles to a `cdylib` WASM module. All public functions are exported via `wasm-bindgen`. Input and output use JSON strings to cross the WASM boundary. Errors are always returned as `{"error": "message"}` rather than panicking.

### t[plugin.urgency-sort]
`sort_by_urgency(tasks_json: &str) -> String`

Input: JSON array of Task objects.
Output: Same array sorted by urgency score descending (see `t[task.urgency]`).
Errors returned as `{"error": "..."}`.

### t[plugin.query]
`execute_query(tasks_json: &str, query_json: &str) -> String`

Input:
- `tasks_json`: JSON array of Task objects
- `query_json`: JSON object describing a Query (filters, sort, group, limit)

Output: JSON array of Task objects (or grouped result object if grouping is specified).
Errors returned as `{"error": "..."}`.

### t[plugin.parse-task]
`parse_task(frontmatter_yaml: &str, body: &str) -> String`

Parses a task from raw YAML frontmatter and Markdown body.
Output: JSON Task object, or `{"error": "..."}` on parse failure.

### t[plugin.serialize-task]
`serialize_task(task_json: &str) -> String`

Serializes a Task JSON object back to YAML frontmatter.
Output: YAML string, or `{"error": "..."}`.

### t[plugin.urgency-score]
`urgency_score(task_json: &str) -> String`

Computes and returns the urgency score for a single task.
Output: `{"score": <number>}` or `{"error": "..."}`.

### t[plugin.validate-task]
`validate_task(task_json: &str) -> String`

Validates a Task JSON object against the schema.
Output: `{"valid": true}` or `{"valid": false, "errors": ["..."]}`.
