---
title: Framework catalog
description: How croft tells which framework a process runs, and how to extend it.
sidebar:
  order: 4
---

A process's framework comes from the libraries it loaded, listed in `/proc/<pid>/maps`, or from its arguments in `/proc/<pid>/cmdline` for frameworks that map no library of their own. The catalog is a JSON file, built into the binary and replaceable.

| Framework | Matched by |
|---|---|
| sglang | directory `/sgl_kernel/` |
| vllm | directory `/vllm/` |
| ollama | library `ollama` (its binary), directory `/lib/ollama/` |
| ray | directory `/ray/` |
| spark | cmdline `org.apache.spark`, `pyspark` |
| transformers | directories `/tokenizers/`, `/safetensors/` |
| pytorch | `libtorch_cpu.so`, `libtorch_cuda.so` |
| tensorflow | `libtensorflow_framework.so`, `libtensorflow_cc.so`, `_pywrap_tensorflow_internal.so` |
| jax | `xla_extension.so`, directory `/jaxlib/` |
| onnxruntime | `libonnxruntime`, `onnxruntime_pybind11_state` |
| xgboost | `libxgboost.so` |
| scikit-learn | directory `/sklearn/` |
| pandas | directory `/pandas/` |

## Order

A process matching several entries takes the earliest. Serving and orchestration come first, then transformers, then the ML frameworks, pandas last: a vLLM server also loads PyTorch and is labelled `vllm`, a Hugging Face training run is `transformers`, and only a job that is pandas alone is `pandas`.

## What maps cannot see

Only mapped files appear in `maps`: shared libraries and compiled extensions. Pure-Python code is read, not mapped, so:

- transformers is seen through `tokenizers` and `safetensors`, the Rust libraries it always loads;
- pyspark is pure Python and the JVM reads jars without mapping them, so spark is matched on the command line.

Libraries every Python process loads, such as numpy, do not tell tasks apart and stay out of the catalog.

## When it is read

Once per new process, then again each poll while it has none, for up to a minute: frameworks are often imported after start. A tgid reused by a new process starts over.

## Extending it

Copy `collector/collector/frameworks.json`, edit it, and point the collector at it:

```sh
croft run --frameworks /etc/croft/frameworks.json     # or CROFT_FRAMEWORKS=...
```

```json
[
  { "name": "vllm", "directories": ["/vllm/"] },
  { "name": "spark", "cmdline": ["org.apache.spark", "pyspark"] },
  { "name": "pytorch", "libraries": ["libtorch_cpu.so", "libtorch_cuda.so"] }
]
```

| Key | Matches |
|---|---|
| `libraries` | the start of a mapped file's name |
| `directories` | anywhere in a mapped file's path |
| `cmdline` | anywhere in the process's arguments |

Each entry needs a unique `name` and at least one pattern; a file that breaks either rule is refused at start. The file replaces the built-in catalog rather than adding to it.
