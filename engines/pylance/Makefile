VENV_DIR := $(abspath ../../.venv)
PYTHON := $(VENV_DIR)/bin/python
PIP := $(PYTHON) -m pip
REQS := $(abspath ../../requirements.txt)
DEPS_STAMP := .deps

clean:
	@rm -rf idx

compile: deps

index: compile
	@echo "--- Indexing pylance ---"
	@$(PYTHON) index.py idx < $(CORPUS)

serve: compile
	@$(PYTHON) serve.py idx

deps: $(DEPS_STAMP)

$(DEPS_STAMP): $(REQS) $(PYTHON)
	@$(PIP) install -r $(REQS) 1>&2
	@touch $(DEPS_STAMP)

$(PYTHON):
	@python3 -m venv $(VENV_DIR)
	@$(PIP) install --upgrade pip
