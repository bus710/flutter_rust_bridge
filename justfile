# To use this file, install Just: cargo install just

frb_bin := "frb_codegen/target/debug/flutter_rust_bridge_codegen"
frb_pure := "frb_example/pure_dart"
frb_flutter := "frb_example/with_flutter"
line_length := "120"

default: gen-bridge lint

alias b := build
build:
    cd frb_codegen && cargo build

alias g := gen-bridge
gen-bridge: build
    {{frb_bin}} -r {{frb_pure}}/rust/src/api.rs \
                -d {{frb_pure}}/dart/lib/bridge_generated.dart
    {{frb_bin}} -r {{frb_flutter}}/rust/src/api.rs \
                -d {{frb_flutter}}/lib/bridge_generated.dart

alias l := lint
lint:
    dart format --fix -l {{line_length}} {{frb_pure}}/dart/lib/bridge_generated.dart
    dart format --fix -l {{line_length}} {{frb_flutter}}/lib/bridge_generated.dart

# vim:expandtab:tabstop=4:shiftwidth=4
