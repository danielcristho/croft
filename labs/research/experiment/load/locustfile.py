"""Load on the webapp: constant users from the CLI, or steps from LOAD_STEPS."""

import os

from locust import HttpUser, LoadTestShape, between, task

STEPS = [int(users) for users in os.environ.get("LOAD_STEPS", "").split(",") if users]
STEP_SECONDS = int(os.environ.get("STEP_SECONDS", "120"))
SPAWN_RATE = float(os.environ.get("SPAWN_RATE", "5"))


class WebUser(HttpUser):
    wait_time = between(0.01, 0.05)

    @task
    def cpu(self):
        self.client.get("/cpu")


if STEPS:

    # StepShape holds each user count for STEP_SECONDS, so every rise in load is a moment that can be timed.
    class StepShape(LoadTestShape):
        def tick(self):
            step = int(self.get_run_time() // STEP_SECONDS)
            if step >= len(STEPS):
                return None
            return STEPS[step], SPAWN_RATE
