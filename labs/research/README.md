# croft Research — Lab Workloads

This folder holds the workloads and the harness that croft's experiments run on the k3s lab cluster.

The cluster itself is provisioned from [`labs/k3s-cluster`](../k3s-cluster).

## Directory structure

```text
research/
├── README.md
└── experiment/
    ├── README.md            the harness: suites, commands, output
    ├── RUNNING.md           the procedure: comparing the HPA and croft
    └── workloads/
        └── README.md        the applications under test
```

---

# 1. Order of work

A complete study follows this order:

```text
provision the cluster          labs/k3s-cluster/README.md
       |
       v
deploy the applications        experiment/workloads/README.md
       |
       v
calibrate each application     experiment/RUNNING.md, section 8
       |
       v
one burst per arm              experiment/RUNNING.md, section 12
       |
       v
the comparison series          experiment/RUNNING.md, section 16
       |
       v
collect croft's datasets       labs/k3s-cluster/README.md
```

Do not begin with a series. First establish that each application, its calibration and each arm are valid.
