"""Controlled GPU workloads whose label is known, for the croft dataset."""

import argparse
import json
import multiprocessing as mp
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import torch
from torch import nn

# Consts
DEVICE = "cuda"
VOCAB = 8000
SERVE_PORT = 8000


def cnn(width: int) -> nn.Module:
    return nn.Sequential(
        nn.Conv2d(3, width, 3, padding=1), nn.ReLU(), nn.MaxPool2d(2),
        nn.Conv2d(width, width * 2, 3, padding=1), nn.ReLU(), nn.MaxPool2d(2),
        nn.Conv2d(width * 2, width * 4, 3, padding=1), nn.ReLU(), nn.AdaptiveAvgPool2d(1),
        nn.Flatten(), nn.Linear(width * 4, 100),
    ).to(DEVICE)


class TinyLM(nn.Module):
    def __init__(self, width: int, layers: int):
        super().__init__()
        self.embed = nn.Embedding(VOCAB, width)
        block = nn.TransformerEncoderLayer(width, nhead=8, dim_feedforward=width * 4, batch_first=True)
        self.blocks = nn.TransformerEncoder(block, layers)
        self.head = nn.Linear(width, VOCAB)

    def forward(self, tokens: torch.Tensor) -> torch.Tensor:
        mask = nn.Transformer.generate_square_subsequent_mask(tokens.size(1), device=tokens.device)
        return self.head(self.blocks(self.embed(tokens), mask=mask, is_causal=True))


def images(batch: int) -> tuple[torch.Tensor, torch.Tensor]:
    return torch.randn(batch, 3, 64, 64), torch.randint(0, 100, (batch,))


def train_cnn(p: dict, deadline: float) -> None:
    model = cnn(p["width"])
    optimizer = torch.optim.SGD(model.parameters(), lr=0.01, momentum=0.9)
    while time.time() < deadline:
        x, y = images(p["batch"])
        loss = nn.functional.cross_entropy(model(x.to(DEVICE)), y.to(DEVICE))
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()


def train_transformer(p: dict, deadline: float) -> None:
    model = TinyLM(p["width"], p["layers"]).to(DEVICE)
    optimizer = torch.optim.AdamW(model.parameters(), lr=3e-4)
    while time.time() < deadline:
        tokens = torch.randint(0, VOCAB, (p["batch"], 129), device=DEVICE)
        logits = model(tokens[:, :-1])
        loss = nn.functional.cross_entropy(logits.reshape(-1, VOCAB), tokens[:, 1:].reshape(-1))
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()


@torch.no_grad()
def infer_batch_cnn(p: dict, deadline: float) -> None:
    model = cnn(p["width"]).eval()
    while time.time() < deadline:
        x, _ = images(p["batch"])
        model(x.to(DEVICE)).argmax(dim=1).cpu()


@torch.no_grad()
def infer_generate_lm(p: dict, deadline: float) -> None:
    model = TinyLM(p["width"], p["layers"]).to(DEVICE).eval()
    while time.time() < deadline:
        tokens = torch.randint(0, VOCAB, (p["batch"], 16), device=DEVICE)
        for _ in range(p["new_tokens"]):
            next_token = model(tokens)[:, -1].argmax(dim=-1, keepdim=True)
            tokens = torch.cat([tokens, next_token], dim=1)
        tokens.cpu()


# Starts a local HTTP server that serves a CNN model, and sends requests to it until the deadline.
def serve_http(p: dict, deadline: float) -> None:
    model = cnn(p["width"]).eval()

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            batch = json.loads(self.rfile.read(int(self.headers["Content-Length"])))["batch"]
            with torch.no_grad():
                classes = model(images(batch)[0].to(DEVICE)).argmax(dim=1).tolist()
            body = json.dumps({"classes": classes}).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_) -> None:
            pass

    server = ThreadingHTTPServer(("127.0.0.1", SERVE_PORT), Handler)
    # The client is a separate CPU-only process, so only the server shows up as GPU activity.
    client = mp.get_context("spawn").Process(target=send_requests, args=(p, deadline))
    client.start()
    server.timeout = 0.5
    while time.time() < deadline:
        server.handle_request()
    client.join()


# Sends HTTP requests to the local server until the deadline, with a think time between requests.
def send_requests(p: dict, deadline: float) -> None:
    body = json.dumps({"batch": p["batch"]}).encode()
    while time.time() < deadline:
        request = urllib.request.Request(f"http://127.0.0.1:{SERVE_PORT}", data=body, method="POST")
        try:
            urllib.request.urlopen(request, timeout=5).read()
        except OSError:
            pass
        time.sleep(p["think_ms"] / 1000)


def gpu_idle(p: dict, deadline: float) -> None:
    held = torch.randn(p["width"], p["width"], device=DEVICE)
    while time.time() < deadline:
        time.sleep(1)
    del held


SCENARIOS = {
    "train-cnn": train_cnn,
    "train-transformer": train_transformer,
    "infer-batch-cnn": infer_batch_cnn,
    "infer-generate-lm": infer_generate_lm,
    "serve-http": serve_http,
    "gpu-idle": gpu_idle,
}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("scenario", choices=SCENARIOS)
    parser.add_argument("--seconds", type=float, required=True)
    parser.add_argument("--params", type=json.loads, default={})
    args = parser.parse_args()
    torch.manual_seed(0)
    SCENARIOS[args.scenario](args.params, time.time() + args.seconds)
    torch.cuda.synchronize()


if __name__ == "__main__":
    main()
