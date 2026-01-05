clean:
	rm -fr idx
	rm -fr target

compile: target/release/build_index target/release/do_query

index: idx

serve: target/release/do_query
	@target/release/do_query idx

target/release/%:
	@echo "\n\n\n--- Building pylance (Rust) ---"
	@cargo build --release --bin $(notdir $@)

idx: target/release/build_index
	@echo "\n\n\n---- Indexing pylance (Rust) ----"
	@mkdir -p idx
	@target/release/build_index idx < $(CORPUS)
