import os

os.environ.setdefault("COMMANDS", "TOP_100")

import src.client as client


def test_run_warmup_runs_once_when_time_non_positive():
    calls = []

    def drive_fn(queries, client_obj, command):
        calls.append((queries, client_obj, command))
        if False:
            yield None

    times = [0, 0]
    idx = {"i": 0}

    def now_fn():
        current = times[idx["i"]]
        if idx["i"] < len(times) - 1:
            idx["i"] += 1
        return current

    progress = []

    def progress_fn(value, **kwargs):
        progress.append(value)

    rounds = client.run_warmup(
        ["q1", "q2"],
        None,
        "TOP_100",
        warmup_time=0,
        now_fn=now_fn,
        drive_fn=drive_fn,
        progress_fn=progress_fn,
    )

    assert rounds == 1
    assert len(calls) == 1
    assert progress[-1] == 1.0


def test_run_warmup_runs_until_time_elapsed():
    calls = []

    def drive_fn(queries, client_obj, command):
        calls.append((queries, client_obj, command))
        if False:
            yield None

    times = [0, 4, 9, 12]
    idx = {"i": 0}

    def now_fn():
        current = times[idx["i"]]
        if idx["i"] < len(times) - 1:
            idx["i"] += 1
        return current

    rounds = client.run_warmup(
        ["q1"],
        None,
        "TOP_100",
        warmup_time=10,
        now_fn=now_fn,
        drive_fn=drive_fn,
        progress_fn=lambda *args, **kwargs: None,
    )

    assert rounds == 3
    assert len(calls) == 3
