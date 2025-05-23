SHELL := /bin/bash
NAME = "horust"
VERSION = "v0.2.0"
DOCKER_REMOTE_REPO = "federicoponzi"
LOCAL_DEV_CONTAINER_NAME = "docker-horust"
LOCAL_DEV_WORKDIR = "/usr/src/Horust"
REPO_HOME := $(shell git rev-parse --show-toplevel)
GIT_COMMIT := $(shell git rev-parse HEAD)
GIT_BRANCH := $(shell git rev-parse --abbrev-ref HEAD)
APP_NAME := "horust"

# Common
COMMON_DOCKER_PARAMS := --build-arg GIT_COMMIT="$(GIT_COMMIT)" --build-arg GIT_BRANCH="$(GIT_BRANCH)"
.PHONY: help
.DEFAULT_GOAL := help

help: ## This help.
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# General docker tasks
build: ## Build the container
	docker build -t $(DOCKER_REMOTE_REPO)/$(APP_NAME):$(VERSION) $(COMMON_DOCKER_PARAMS) .

build-nofeatures: ## Build the container without http requests.
	docker build -t $(DOCKER_REMOTE_REPO)/$(APP_NAME)_nofeatures:$(VERSION) $(COMMON_DOCKER_PARAMS) --build-arg CARGO_PARAMS="--no-default-features" .

run: ## Run container on port configured in `config.env`
	docker run -it --rm --env HORUST_LOG=debug -v $(REPO_HOME)/examples/services/longrunning/:/etc/horust/services/ --name="$(NAME)" $(NAME):$(VERSION)

run-bash: ## Run bash with horust
	docker run -it --rm --env HORUST_LOG=debug --name="$(NAME)" $(DOCKER_REMOTE_REPO)/$(APP_NAME):$(VERSION) -- /bin/bash

stop: ## Stop and remove a running container
	docker stop $(NAME)

# Docker publishing tasks
publish: build publish-latest publish-version ## publish the `{version}` ans `latest` tagged containers to ECR

publish-latest: tag-latest ## publish the `latest` tagged container
	@echo 'publish latest to $(DOCKER_REMOTE_REPO)'
	docker push $(DOCKER_REMOTE_REPO)/$(APP_NAME):latest

publish-version: ## publish the `{version}` tagged container to ECR
	@echo 'publish $(VERSION) to $(DOCKER_REMOTE_REPO)'
	docker push $(DOCKER_REMOTE_REPO)/$(APP_NAME):$(VERSION)

tag-latest: ## tags the latest container with the version listed above
	@echo 'create tag latest'
	docker tag $(DOCKER_REMOTE_REPO)/$(APP_NAME):$(VERSION) $(DOCKER_REMOTE_REPO)/$(APP_NAME):latest

version: ## output to version
	@echo $(VERSION)

# Docker local development tasks
## Dargo == Docker Cargo
dargo: ## Run a cargo command inside the container
	@# If the dargo container does not exist, create it
	@if [ -z $(shell docker ps --format "{{.ID}}" --filter "name=$(LOCAL_DEV_CONTAINER_NAME)") ]; then make dargo-run-container; fi
	@# Run a command inside the container
	docker exec -ti $(LOCAL_DEV_CONTAINER_NAME) cargo $(COMMAND)

dargo-run-container: ## Runs a Rust container with the pwd (i.e. current folder) bind-mounted to it
	@echo 'running interactive rust container for local development'
	@docker run \
	--detach \
	--tty \
 	--name $(LOCAL_DEV_CONTAINER_NAME) \
 	--workdir $(LOCAL_DEV_WORKDIR) \
 	--mount type=bind,source="$(shell pwd)",target=$(LOCAL_DEV_WORKDIR) \
 	rust:1
