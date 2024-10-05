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
infer-mli --root-dir ~/projects/ocaml/infer-mli --file ~/projects/ocaml/infer-mli/src/infer_mli.ml
```

This will create a file `infer_mli.mli` in the root directory of the project.

## License

MIT
