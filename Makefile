NAME = "horust"
VERSION = "v0.0.1"
DOCKER_REPO = "horust"
REPO_HOME := $(shell git rev-parse --show-toplevel)

.PHONY: help

help: ## This help.
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

.DEFAULT_GOAL := help

# DOCKER TASKS
# Build the container
build: ## Build the container
	docker build -t $(NAME):$(VERSION) .

build-nofeatures: ## Build the container without http requests.
	docker build -t $(NAME):$(VERSION) --build-arg CARGO_PARAMS="--no-default-features" .

build-nc: ## Build the container without caching
	docker build --no-cache -t $(NAME) .

run: ## Run container on port configured in `config.env`
	docker run -it --rm --env HORUST_LOG=debug -v $(REPO_HOME)/examples/services/longrunning/:/etc/horust/services/ --name="$(NAME)" $(NAME):$(VERSION)

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