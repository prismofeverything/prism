//! Reaction rules for the MAPK BRS.
//!
//! Each rule is a parametric bigraph rewrite: the redex describes
//! both place-graph nesting and link-graph wiring; the reactum rewrites
//! both. Rates are propensity coefficients under Gillespie SSA:
//!
//! ```text
//! propensity(R) = R.rate * |matches(R)|
//! ```

use prism_schema::{Pattern, ReactionRule};

/// Free MEK + free ERK in the same compartment form the Michaelis
/// complex (drawn here as a shared link-graph edge).
pub fn rule_phosphorylate() -> ReactionRule {
    ReactionRule::new(
        Pattern::map([(
            "compartment",
            Pattern::sort(
                "Compartment",
                [
                    (
                        "enzyme",
                        Pattern::sort("MEK", [("outputs", Pattern::absent())]),
                    ),
                    (
                        "substrate",
                        Pattern::sort(
                            "ERK",
                            [
                                ("name", Pattern::site()),
                                ("outputs", Pattern::absent()),
                            ],
                        ),
                    ),
                    ("bystanders", Pattern::site()),
                ],
            ),
        )]),
        Pattern::map([(
            "compartment",
            Pattern::sort(
                "Compartment",
                [
                    (
                        "enzyme",
                        Pattern::sort(
                            "MEK",
                            [(
                                "outputs",
                                Pattern::map([(
                                    "enzyme_port",
                                    Pattern::link_var("bond"),
                                )]),
                            )],
                        ),
                    ),
                    (
                        "substrate",
                        Pattern::sort(
                            "pERK",
                            [
                                ("name", Pattern::site()),
                                (
                                    "outputs",
                                    Pattern::map([(
                                        "substrate_port",
                                        Pattern::link_var("bond"),
                                    )]),
                                ),
                            ],
                        ),
                    ),
                    ("bystanders", Pattern::site()),
                ],
            ),
        )]),
    )
    .with_instantiation([("bystanders", "bystanders"), ("name", "name")])
    .with_rate(2.0)
    .with_label("phosphorylate")
}

/// MEK·pERK complex dissociates back to free MEK + free pERK (both
/// still inside the compartment).
pub fn rule_dissociate() -> ReactionRule {
    ReactionRule::new(
        Pattern::map([(
            "compartment",
            Pattern::sort(
                "Compartment",
                [
                    (
                        "enzyme",
                        Pattern::sort(
                            "MEK",
                            [(
                                "outputs",
                                Pattern::map([(
                                    "enzyme_port",
                                    Pattern::link_var("bond"),
                                )]),
                            )],
                        ),
                    ),
                    (
                        "substrate",
                        Pattern::sort(
                            "pERK",
                            [
                                ("name", Pattern::site()),
                                (
                                    "outputs",
                                    Pattern::map([(
                                        "substrate_port",
                                        Pattern::link_var("bond"),
                                    )]),
                                ),
                            ],
                        ),
                    ),
                    ("bystanders", Pattern::site()),
                ],
            ),
        )]),
        Pattern::map([(
            "compartment",
            Pattern::sort(
                "Compartment",
                [
                    ("enzyme", Pattern::sort("MEK", Vec::<(&str, Pattern)>::new())),
                    (
                        "substrate",
                        Pattern::sort("pERK", [("name", Pattern::site())]),
                    ),
                    ("bystanders", Pattern::site()),
                ],
            ),
        )]),
    )
    .with_instantiation([("bystanders", "bystanders"), ("name", "name")])
    .with_rate(0.5)
    .with_label("dissociate")
}

/// Nuclear pERK is dephosphorylated back to ERK.
pub fn rule_dephosphorylate() -> ReactionRule {
    ReactionRule::new(
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "inner",
                        Pattern::sort(
                            "Compartment",
                            [
                                ("kind", Pattern::sort("Nucleus", Vec::<(&str, Pattern)>::new())),
                                (
                                    "substrate",
                                    Pattern::sort(
                                        "pERK",
                                        [
                                            ("name", Pattern::site()),
                                            ("outputs", Pattern::absent()),
                                        ],
                                    ),
                                ),
                                ("inner_rest", Pattern::site()),
                            ],
                        ),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "inner",
                        Pattern::sort(
                            "Compartment",
                            [
                                ("kind", Pattern::sort("Nucleus", Vec::<(&str, Pattern)>::new())),
                                (
                                    "substrate",
                                    Pattern::sort("ERK", [("name", Pattern::site())]),
                                ),
                                ("inner_rest", Pattern::site()),
                            ],
                        ),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
    )
    .with_instantiation([
        ("outer_rest", "outer_rest"),
        ("inner_rest", "inner_rest"),
        ("name", "name"),
    ])
    .with_rate(0.4)
    .with_label("dephosphorylate")
}

/// Free ERK descends from cytoplasm into a child compartment.
pub fn rule_translocate_erk_in() -> ReactionRule {
    ReactionRule::new(
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "substrate",
                        Pattern::sort("ERK", [("name", Pattern::site())]),
                    ),
                    (
                        "inner",
                        Pattern::sort("Compartment", [("inner_rest", Pattern::site())]),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "inner",
                        Pattern::sort(
                            "Compartment",
                            [
                                ("inner_rest", Pattern::site()),
                                (
                                    "substrate",
                                    Pattern::sort("ERK", [("name", Pattern::site())]),
                                ),
                            ],
                        ),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
    )
    .with_instantiation([
        ("outer_rest", "outer_rest"),
        ("inner_rest", "inner_rest"),
        ("name", "name"),
    ])
    .with_rate(1.0)
    .with_label("translocate_erk_in")
}

/// Free ERK ascends from a child compartment back into cytoplasm.
pub fn rule_translocate_erk_out() -> ReactionRule {
    ReactionRule::new(
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "inner",
                        Pattern::sort(
                            "Compartment",
                            [
                                (
                                    "substrate",
                                    Pattern::sort("ERK", [("name", Pattern::site())]),
                                ),
                                ("inner_rest", Pattern::site()),
                            ],
                        ),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "inner",
                        Pattern::sort("Compartment", [("inner_rest", Pattern::site())]),
                    ),
                    (
                        "substrate",
                        Pattern::sort("ERK", [("name", Pattern::site())]),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
    )
    .with_instantiation([
        ("outer_rest", "outer_rest"),
        ("inner_rest", "inner_rest"),
        ("name", "name"),
    ])
    .with_rate(1.0)
    .with_label("translocate_erk_out")
}

/// Active nuclear import of free phospho-ERK (fast).
pub fn rule_translocate_perk_in() -> ReactionRule {
    ReactionRule::new(
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "substrate",
                        Pattern::sort(
                            "pERK",
                            [
                                ("name", Pattern::site()),
                                ("outputs", Pattern::absent()),
                            ],
                        ),
                    ),
                    (
                        "inner",
                        Pattern::sort(
                            "Compartment",
                            [
                                ("kind", Pattern::sort("Nucleus", Vec::<(&str, Pattern)>::new())),
                                ("inner_rest", Pattern::site()),
                            ],
                        ),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "inner",
                        Pattern::sort(
                            "Compartment",
                            [
                                ("kind", Pattern::sort("Nucleus", Vec::<(&str, Pattern)>::new())),
                                ("inner_rest", Pattern::site()),
                                (
                                    "substrate",
                                    Pattern::sort("pERK", [("name", Pattern::site())]),
                                ),
                            ],
                        ),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
    )
    .with_instantiation([
        ("outer_rest", "outer_rest"),
        ("inner_rest", "inner_rest"),
        ("name", "name"),
    ])
    .with_rate(2.0)
    .with_label("translocate_perk_in")
}

/// Slow nuclear export / leak of free phospho-ERK.
pub fn rule_translocate_perk_out() -> ReactionRule {
    ReactionRule::new(
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "inner",
                        Pattern::sort(
                            "Compartment",
                            [
                                ("kind", Pattern::sort("Nucleus", Vec::<(&str, Pattern)>::new())),
                                (
                                    "substrate",
                                    Pattern::sort(
                                        "pERK",
                                        [
                                            ("name", Pattern::site()),
                                            ("outputs", Pattern::absent()),
                                        ],
                                    ),
                                ),
                                ("inner_rest", Pattern::site()),
                            ],
                        ),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
        Pattern::map([(
            "outer",
            Pattern::sort(
                "Compartment",
                [
                    ("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new())),
                    (
                        "inner",
                        Pattern::sort(
                            "Compartment",
                            [
                                ("kind", Pattern::sort("Nucleus", Vec::<(&str, Pattern)>::new())),
                                ("inner_rest", Pattern::site()),
                            ],
                        ),
                    ),
                    (
                        "substrate",
                        Pattern::sort("pERK", [("name", Pattern::site())]),
                    ),
                    ("outer_rest", Pattern::site()),
                ],
            ),
        )]),
    )
    .with_instantiation([
        ("outer_rest", "outer_rest"),
        ("inner_rest", "inner_rest"),
        ("name", "name"),
    ])
    .with_rate(0.1)
    .with_label("translocate_perk_out")
}

/// All seven rules.
pub fn mapk_rules() -> Vec<ReactionRule> {
    vec![
        rule_phosphorylate(),
        rule_dissociate(),
        rule_dephosphorylate(),
        rule_translocate_erk_in(),
        rule_translocate_erk_out(),
        rule_translocate_perk_in(),
        rule_translocate_perk_out(),
    ]
}
