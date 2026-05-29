"""Process classes hosted by the server.

Each declares its ports as **bigraph-schema type strings** (e.g. ``"map[float]"``)
so prism's ``RestProcess`` reconstructs the real ``Schema`` across the bridge —
the same type system on both ends. ``update(state, interval)`` returns the new
port values, mirroring prism's native ``Process`` trait.

``EulerIntegrator`` is the Python counterpart of prism's native integrators: it
integrates a mass-action CRN, and (on the prism side) a contracted wrapper
declares it ``fulfills DeterministicMassAction``. COPASI/Tellurium wrappers
(``basico`` / ``libroadrunner``) slot in alongside it with the SAME port types.
"""

from typing import Any, Dict, List


def propensities(network: Dict[str, Any], x: Dict[str, float]) -> List[float]:
    """Mass-action rate of each reaction: k * prod(x[s] ** coeff)."""
    out = []
    for r in network.get("reactions", []):
        v = float(r.get("k", 0.0))
        for s, coeff in r.get("reactants", {}).items():
            v *= x.get(s, 0.0) ** coeff
        out.append(v)
    return out


def derivatives(network: Dict[str, Any], x: Dict[str, float]) -> Dict[str, float]:
    """dx/dt for every species under mass action."""
    dx = {s: 0.0 for s in network.get("species", [])}
    for r, v in zip(network.get("reactions", []), propensities(network, x)):
        for s, coeff in r.get("reactants", {}).items():
            dx[s] = dx.get(s, 0.0) - coeff * v
        for s, coeff in r.get("products", {}).items():
            dx[s] = dx.get(s, 0.0) + coeff * v
    return dx


def crn_to_sbml(network):
    """Generate SBML (L3v2) from our canonical CRN. We GENERATE it; the engines
    PARSE it — so the agreement demo feeds every engine the SAME model in ONE
    config form (`network`), and COPASI/Tellurium stay the SBML parsers."""
    species = network.get("species", [])
    reactions = network.get("reactions", [])

    def refs(stoich):
        return "".join(
            f'<speciesReference species="{s}" stoichiometry="{c}" constant="true"/>'
            for s, c in stoich.items()
        )

    species_xml = "".join(
        f'<species id="{s}" compartment="c" initialConcentration="0" '
        f'hasOnlySubstanceUnits="false" boundaryCondition="false" constant="false"/>'
        for s in species
    )
    params_xml = "".join(
        f'<parameter id="k{i}" value="{r.get("k", 0.0)}" constant="true"/>'
        for i, r in enumerate(reactions)
    )
    reactions_xml = ""
    for i, r in enumerate(reactions):
        reactants = r.get("reactants", {})
        # mass-action rate: k_i * prod(reactant ** stoichiometry)
        factors = "".join(
            f"<ci>{s}</ci>" for s, c in reactants.items() for _ in range(int(c))
        )
        reactions_xml += (
            f'<reaction id="R{i}" reversible="false">'
            f"<listOfReactants>{refs(reactants)}</listOfReactants>"
            f'<listOfProducts>{refs(r.get("products", {}))}</listOfProducts>'
            f'<kineticLaw><math xmlns="http://www.w3.org/1998/Math/MathML">'
            f"<apply><times/><ci>k{i}</ci>{factors}</apply>"
            f"</math></kineticLaw></reaction>"
        )
    return (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<sbml xmlns="http://www.sbml.org/sbml/level3/version2/core" level="3" version="2">'
        '<model id="crn">'
        '<listOfCompartments><compartment id="c" size="1" constant="true"/></listOfCompartments>'
        f"<listOfSpecies>{species_xml}</listOfSpecies>"
        f"<listOfParameters>{params_xml}</listOfParameters>"
        f"<listOfReactions>{reactions_xml}</listOfReactions>"
        "</model></sbml>"
    )


def sbml_from_config(config):
    """The model as SBML — generated from a CRN `network`, or passed through as
    `sbml`. We never PARSE SBML; the engines do."""
    config = config or {}
    if config.get("sbml"):
        return config["sbml"]
    return crn_to_sbml(config.get("network", {}))


class EulerIntegrator:
    """Forward-Euler over a mass-action CRN.

    config: ``{"network": {"species": [...], "reactions": [{reactants, products, k}]}}``
    The ``state`` port carries the species → amount map.
    """

    def __init__(self, config: Dict[str, Any]):
        self.network = (config or {}).get("network", {})

    def inputs(self) -> Dict[str, str]:
        return {"state": "map[float]"}

    def outputs(self) -> Dict[str, str]:
        return {"state": "map[float]"}

    def update(self, state: Dict[str, Any], interval: float) -> Dict[str, Any]:
        x = dict((state or {}).get("state", {}))
        dx = derivatives(self.network, x)
        species = self.network.get("species", list(x.keys()))
        nxt = {s: x.get(s, 0.0) + interval * dx.get(s, 0.0) for s in species}
        return {"state": nxt}


class CopasiCvode:
    """COPASI (CVODE) over an SBML model, via ``basico``.

    config: ``{"sbml": "<SBML content>"}`` — COPASI parses it; we never do. On
    the prism side a contracted wrapper declares this ``fulfills
    DeterministicMassAction``. The ``state`` port carries species → concentration.
    """

    def __init__(self, config):
        import os
        import tempfile

        import basico

        self._basico = basico
        sbml = sbml_from_config(config)
        # load_model takes a path; write the content to a temp file (works on any
        # basico version, and keeps the model addressable by content over the wire).
        fd, path = tempfile.mkstemp(suffix=".xml")
        with os.fdopen(fd, "w") as f:
            f.write(sbml)
        self.dm = basico.load_model(path)
        os.unlink(path)
        self.species = list(basico.get_species(model=self.dm).index)

    def inputs(self):
        return {"state": "map[float]"}

    def outputs(self):
        return {"state": "map[float]"}

    def update(self, state, interval):
        b = self._basico
        x = (state or {}).get("state", {})
        for s, v in x.items():
            b.set_species(s, initial_concentration=float(v), model=self.dm)
        df = b.run_time_course(duration=float(interval), model=self.dm)
        last = df.iloc[-1]
        out = {}
        for s in self.species:
            # basico labels concentration columns by species name (sometimes "[S]").
            if s in last.index:
                out[s] = float(last[s])
            elif f"[{s}]" in last.index:
                out[s] = float(last[f"[{s}]"])
        return {"state": out}


class RoadRunnerCvode:
    """Tellurium's engine (``libroadrunner``, CVODE) over an SBML model.

    config: ``{"sbml": "<SBML content>"}`` — roadrunner parses the content
    directly. fulfills (prism side) DeterministicMassAction, with the SAME port
    types as COPASI, so the two are interchangeable under the contract.
    """

    def __init__(self, config):
        import roadrunner

        sbml = sbml_from_config(config)
        self.rr = roadrunner.RoadRunner(sbml)
        self.species = list(self.rr.model.getFloatingSpeciesIds())

    def inputs(self):
        return {"state": "map[float]"}

    def outputs(self):
        return {"state": "map[float]"}

    def update(self, state, interval):
        x = (state or {}).get("state", {})
        # Seed the current concentrations, then integrate one tick from there.
        for s, v in x.items():
            self.rr[s] = float(v)
        self.rr.simulate(0.0, float(interval), 2)
        return {"state": {s: float(self.rr[s]) for s in self.species}}


# Process classes the server can host, keyed by the name prism addresses them by.
REGISTRY = {
    "EulerIntegrator": EulerIntegrator,
    "CopasiCvode": CopasiCvode,
    "RoadRunnerCvode": RoadRunnerCvode,
}
