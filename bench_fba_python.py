#!/usr/bin/env python3
"""Benchmark cobrapy FBA solve time across different models."""

import time
import cobra

MODELS = [
    ("e_coli_core", "textbook", 10000),
    ("iNF517",      "iNF517",   2000),
    ("iJN746",      "iJN746",   1000),
    ("iCN900",      "iCN900",   1000),
    ("iMM904",      "iMM904",   500),
    ("iAF1260",     "iAF1260",  200),
]

print("FBA Benchmark: raw LP solve time (Python/cobrapy)")
print("-" * 100)

for name, model_id, n_solves in MODELS:
    try:
        t0 = time.perf_counter()
        if model_id == "textbook":
            model = cobra.io.load_model("textbook")
        else:
            model = cobra.io.load_json_model(f"crates/spatio-flux/models/{model_id}.json")
        load_ms = (time.perf_counter() - t0) * 1000

        n_mets = len(model.metabolites)
        n_rxns = len(model.reactions)

        # Cold solve
        t1 = time.perf_counter()
        sol = model.optimize()
        cold_us = (time.perf_counter() - t1) * 1e6
        obj = sol.objective_value

        # Warm solves
        t2 = time.perf_counter()
        for _ in range(n_solves):
            model.optimize()
        total_us = (time.perf_counter() - t2) * 1e6
        avg_us = total_us / n_solves

        print(f"{name:<15} | {n_mets:>4} mets | {n_rxns:>5} rxns | load {load_ms:>8.1f}ms | cold {cold_us:>8.0f}us | avg {avg_us:>8.1f}us ({n_solves} solves) | obj={obj:.4f}")
    except Exception as e:
        print(f"{name:<15} | ERROR: {e}")
