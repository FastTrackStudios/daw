+++
title = "Repository contract"
description = "Tracey-tracked rules the ExampleRepo implementation must hold."
weight = 100
+++

The rules below are linked to the source via `r[impl <id>]` and
`r[verify <id>]` annotations. Run `tracey query validate` to confirm
every rule has an implementation and a test.

r[repo.create.id]
A `create` call MUST generate a new UUID for the row's primary key and
return the materialized record with `created_at` and `updated_at`
populated from the server clock.

r[repo.get.missing]
A `get` call against an unknown UUID MUST return `RepoError::NotFound`.

r[repo.delete.missing]
A `delete` call against an unknown UUID MUST return `RepoError::NotFound`.

r[repo.list.sort.name]
A `list` call with `Sort { field: "name", order: SortOrder::Asc }` MUST
return rows ordered by `name` ascending. The same call against any
backend MUST produce the same row order.

r[repo.list.sort.unknown]
A `list` call with a `Sort { field: <unknown> }` MUST return
`RepoError::InvalidInput`. This is the architect derive's contract for
fields not annotated `#[architect(sortable)]`.

r[repo.update.partial]
An `update` call MUST patch only the fields whose corresponding
`Option<T>` is `Some`. Fields set to `None` MUST be left unchanged.
