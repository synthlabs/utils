TESTING_TOKE ?= toke
TESTING_PNPM ?= pnpm
TESTING_RUN = $(if $(strip $(TESTING_TOKE)),$(TESTING_TOKE) -v ,)$(TESTING_PNPM)

.PHONY: test verify
test:
	$(TESTING_RUN) test

verify:
	$(TESTING_RUN) verify

.PHONY: test-%
test-%:
	$(TESTING_RUN) run test:$(subst -,:,$*)
