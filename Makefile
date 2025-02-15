.PHONY: all docs

all: docs

docs:
	@echo "Analyzing Cargo module..."
	@./utils/docs-generate-cargo-modules-tree.sh

	@echo "Render GraphViz graphs."
	@./utils/docs-render-diagrams.sh
