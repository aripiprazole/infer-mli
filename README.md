# infer-mli

Creates a .mli for ml files in a ocaml project by cli

## Installing

```
cargo install infer-mli
```

## Usage

```
infer-mli --root-dir <path to ocaml project> --file <path to ml file in root dir>
```

## Example

```
infer-mli --root-dir ~/projects/ocaml/infer-mli --file src/infer_mli.ml
```

This will create a file `infer_mli.mli` in the root directory of the project.

## Using with Zed

Add this to your `~/.config/zed/tasks.json`

```json
[
  {
    "label": "generate .mli file",
    "tags": ["ocaml"],
    "allow_concurrent_runs": true,
    "command": "zed $(infer-mli --root-dir $ZED_WORKTREE_ROOT --file $ZED_RELATIVE_FILE)",
    "reveal": "never"
  }
]
```

and this to your `~/.config/zed/keybindings.json`:

```json
[
  {
    "context": "Workspace",
    "bindings": {
      "alt-g": ["task::Spawn", { "task_name": "generate .mli file" }]
    }
  },
]
```

## License

MIT
