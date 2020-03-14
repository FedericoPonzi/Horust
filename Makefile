NAME = "horust"
VERSION = "v0.1.0"
DOCKER_REPO = "horust"
REPO_HOME := $(shell git rev-parse --show-toplevel)
GIT_COMMIT := $(shell git rev-parse HEAD)
GIT_BRANCH := $(shell git rev-parse --abbrev-ref HEAD)
COMMON_DOCKER_PARAMS := --build-arg GIT_COMMIT="$(GIT_COMMIT)" --build-arg GIT_BRANCH="$(GIT_BRANCH)"
.PHONY: help

help: ## This help.
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

.DEFAULT_GOAL := help

# DOCKER TASKS
# Build the container
build: ## Build the container
	docker build -t $(NAME):$(VERSION) $(COMMON_DOCKER_PARAMS) .

build-nofeatures: ## Build the container without http requests.
	docker build -t $(NAME):$(VERSION) $(COMMON_DOCKER_PARAMS) --build-arg CARGO_PARAMS="--no-default-features" .

run: ## Run container on port configured in `config.env`
	docker run -it --rm --env HORUST_LOG=debug -v $(REPO_HOME)/examples/services/longrunning/:/etc/horust/services/ --name="$(NAME)" $(NAME):$(VERSION)

run-bash: ## Run bash with horust
	docker run -it --rm --env HORUST_LOG=debug --name="$(NAME)" $(NAME):$(VERSION) -- /bin/bash


stop: ## Stop and remove a running container
	docker stop $(NAME)

# Docker publish
publish: repo-login publish-latest publish-version ## publish the `{version}` ans `latest` tagged containers to ECR

publish-latest: tag-latest ## publish the `latest` tagged container to ECR
	@echo 'publish latest to $(DOCKER_REPO)'
	docker push $(DOCKER_REPO)/$(APP_NAME):latest

publish-version: tag-version ## publish the `{version}` tagged container to ECR
	@echo 'publish $(VERSION) to $(DOCKER_REPO)'
	docker push $(DOCKER_REPO)/$(APP_NAME):$(VERSION)

version: ## output to version
	@echo $(VERSION)