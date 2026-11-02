"""Load on an application's request path (APP_PATH): constant users from the CLI, or steps from LOAD_STEPS, held or ramped."""

import os

from locust import HttpUser, LoadTestShape, between, task

STEPS = [int(users) for users in os.environ.get("LOAD_STEPS", "").split(",") if users]
STEP_SECONDS = int(os.environ.get("STEP_SECONDS", "120"))
SPAWN_RATE = float(os.environ.get("SPAWN_RATE", "5"))
SHAPE = os.environ.get("LOAD_SHAPE", "step")
APP_PATH = os.environ.get("APP_PATH", "/")


class WebUser(HttpUser):
    wait_time = between(0.01, 0.05)

    @task
    def request(self):
        self.client.get(APP_PATH)


if STEPS and SHAPE == "step":

    # StepShape holds each user count for STEP_SECONDS, so every rise in load is a moment that can be timed.
    class StepShape(LoadTestShape):
        def tick(self):
            step = int(self.get_run_time() // STEP_SECONDS)
            if step >= len(STEPS):
                return None
            return STEPS[step], SPAWN_RATE

elif STEPS and SHAPE == "ramp":

    # ramp_users reaches each step's count one step later than StepShape and holds the last, so both last as long.
    def ramp_users(run_time):
        segment = int(run_time // STEP_SECONDS)
        if segment >= len(STEPS) - 1:
            return STEPS[-1]
        fraction = run_time / STEP_SECONDS - segment
        return max(1, round(STEPS[segment] + fraction * (STEPS[segment + 1] - STEPS[segment])))

    # RampShape adds users one at a time, so the signals rise gradually instead of all at once.
    class RampShape(LoadTestShape):
        def tick(self):
            run_time = self.get_run_time()
            if run_time >= len(STEPS) * STEP_SECONDS:
                return None
            return ramp_users(run_time), SPAWN_RATE
