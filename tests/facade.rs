extern crate allowit_sdk as allowit;

mod research {
    include!("../examples/research.rs");
}
mod approval {
    include!("../examples/approval.rs");
}
mod green {
    include!("../examples/green-investments.rs");
}

#[test]
fn example_policies_are_valid_rust_against_the_facade() {
    let _ = research::evaluate;
    let _ = approval::evaluate;
    let _ = green::evaluate;
}
