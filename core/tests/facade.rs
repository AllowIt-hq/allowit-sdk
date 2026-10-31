extern crate allowit_sdk as allowit;

mod research {
    include!("../../docs/examples/research.rs");
}
mod approval {
    include!("../../docs/examples/approval.rs");
}
mod green {
    include!("../../docs/examples/green-investments.rs");
}

#[test]
fn example_policies_are_valid_rust_against_the_facade() {
    let _ = research::evaluate;
    let _ = approval::evaluate;
    let _ = green::evaluate;
}
