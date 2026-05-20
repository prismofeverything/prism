#!/usr/bin/env python3
"""Run spatioflux_reference_demo for 20s in Python and dump timeseries."""
import os
import sys

# spatio-flux, assumed cloned alongside prism; override with PRISM_SIBLINGS.
_siblings = os.environ.get(
    "PRISM_SIBLINGS",
    os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."),
)
sys.path.insert(0, os.path.join(_siblings, "spatio-flux"))

import json
import numpy as np

from spatio_flux.experiments.figure_B import get_reference_composite_spec

def run_20s():
    from process_bigraph import Composite

    spec = get_reference_composite_spec()
    composite = Composite(spec)

    # Run for 20s, emitting every 1s
    composite.run(20.0)
    results = composite.gather_results()

    # Extract key timeseries
    times = [float(t) for t in results[('global_time',)]]

    # Particle count over time
    particle_counts = []
    for t_idx in range(len(times)):
        particles = results.get(('particles',), [{}])[t_idx]
        if isinstance(particles, dict):
            particle_counts.append(len(particles))
        else:
            particle_counts.append(0)

    # Total mass over time
    total_masses = []
    for t_idx in range(len(times)):
        particles = results.get(('particles',), [{}])[t_idx]
        total = 0.0
        if isinstance(particles, dict):
            for pid, p in particles.items():
                if isinstance(p, dict):
                    mass = p.get('mass', 0.0)
                    if mass is None:
                        mass = 0.0
                    total += float(mass)
        total_masses.append(total)

    # Field values at center
    glucose_center = []
    acetate_center = []
    for t_idx in range(len(times)):
        fields = results.get(('fields',), [{}])
        if t_idx < len(fields) and isinstance(fields[t_idx], dict):
            glc = fields[t_idx].get('glucose', [[0]*10]*10)
            ace = fields[t_idx].get('acetate', [[0]*10]*10)
            if isinstance(glc, (list, np.ndarray)) and len(glc) > 5:
                row = glc[5]
                if isinstance(row, (list, np.ndarray)) and len(row) > 5:
                    glucose_center.append(float(row[5]))
                else:
                    glucose_center.append(0.0)
            else:
                glucose_center.append(0.0)
            if isinstance(ace, (list, np.ndarray)) and len(ace) > 5:
                row = ace[5]
                if isinstance(row, (list, np.ndarray)) and len(row) > 5:
                    acetate_center.append(float(row[5]))
                else:
                    acetate_center.append(0.0)
            else:
                acetate_center.append(0.0)
        else:
            glucose_center.append(0.0)
            acetate_center.append(0.0)

    output = {
        'times': times,
        'particle_counts': particle_counts,
        'total_masses': total_masses,
        'glucose_center_5_5': glucose_center,
        'acetate_center_5_5': acetate_center,
    }

    with open('/tmp/python_20s.json', 'w') as f:
        json.dump(output, f, indent=2)

    print(f"Python: {len(times)} timepoints, final particles={particle_counts[-1]}, final mass={total_masses[-1]:.4f}")
    print(f"  glucose[5,5]: {glucose_center[0]:.4f} -> {glucose_center[-1]:.4f}")
    print(f"  acetate[5,5]: {acetate_center[0]:.4f} -> {acetate_center[-1]:.4f}")

if __name__ == '__main__':
    run_20s()
