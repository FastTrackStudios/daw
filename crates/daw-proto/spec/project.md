# Project Service

The Project service manages project lifecycle operations including creation, opening, saving, and closing projects.

## Overview

The Project service is responsible for:
- Creating new projects
- Opening existing projects
- Saving project state
- Closing projects
- Retrieving project metadata

All DAWs MUST implement the core Project service behaviors.

## Requirements

### Project Lifecycle

r[project.create]
The service MUST support creating a new project with a specified name.

r[project.open]
The service MUST support opening an existing project from a file path.

r[project.save]
The service MUST support saving the current project state.

r[project.close]
The service MUST support closing the current project.

r[project.close.prompt-save]
If the project has unsaved changes, closing SHOULD prompt for save (implementation-specific).

### Project Metadata

r[project.name]
The service MUST provide the current project name.

r[project.path]
The service MUST provide the current project file path.

r[project.modified]
The service MUST indicate whether the project has unsaved changes.

r[project.tracks]
The service MUST provide a list of tracks in the project.

## Error Handling

r[project.error.not-found]
Opening a non-existent project MUST return an appropriate error.

r[project.error.permission]
Permission errors (read/write) MUST be reported clearly.

r[project.error.format]
Invalid project format errors MUST be reported clearly.