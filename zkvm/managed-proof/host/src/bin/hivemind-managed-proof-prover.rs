use std::{
    env,
    ffi::OsString,
    io::{self, Read, Write},
    process::ExitCode,
};

use hivemind_managed_prover_protocol::{
    ManagedProverRequest, ManagedProverResponse, MAX_REQUEST_JSON_BYTES,
};

pub const FAILURE_MESSAGE: &[u8] = b"managed proof generation failed\n";

#[cfg(not(feature = "sidecar-test-harness"))]
use hivemind_managed_proof_zkvm as prover_backend;

#[cfg(feature = "sidecar-test-harness")]
mod prover_backend {
    use hivemind_managed_prover_protocol::{ManagedProverRequest, ManagedProverResponse};

    #[allow(dead_code)]
    pub fn handle_prover_request(_: ManagedProverRequest) -> Result<ManagedProverResponse, ()> {
        Err(())
    }
}

#[cfg_attr(feature = "sidecar-test-harness", allow(dead_code))]
fn main() -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stderr = io::stderr();

    let exit_code = run_main(
        env::args_os(),
        stdin.lock(),
        stdout.lock(),
        stderr.lock(),
        prover_backend::handle_prover_request,
    );

    ExitCode::from(exit_code)
}

pub fn run_main<I, R, O, E, F, ProveError>(
    args: I,
    mut stdin: R,
    mut stdout: O,
    mut stderr: E,
    prove: F,
) -> u8
where
    I: IntoIterator<Item = OsString>,
    R: Read,
    O: Write,
    E: Write,
    F: FnOnce(ManagedProverRequest) -> Result<ManagedProverResponse, ProveError>,
{
    if run_main_inner(args, &mut stdin, &mut stdout, prove).is_err() {
        let _ = stderr.write_all(FAILURE_MESSAGE);
        return 1;
    }
    0
}

fn run_main_inner<I, R, O, F, ProveError>(
    args: I,
    stdin: &mut R,
    stdout: &mut O,
    prove: F,
) -> Result<(), ()>
where
    I: IntoIterator<Item = OsString>,
    R: Read,
    O: Write,
    F: FnOnce(ManagedProverRequest) -> Result<ManagedProverResponse, ProveError>,
{
    let mut args = args.into_iter();
    if args.next().is_none() || args.next().is_some() {
        return Err(());
    }

    let mut input = Vec::new();
    stdin
        .take((MAX_REQUEST_JSON_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .map_err(|_| ())?;

    let request = ManagedProverRequest::from_json_bytes(&input).map_err(|_| ())?;
    let response = prove(request).map_err(|_| ())?;
    let output = response.to_json_bytes().map_err(|_| ())?;
    stdout.write_all(&output).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, ffi::OsString};

    use hivemind_managed_prover_protocol::{
        ManagedProverRequest, ManagedProverResponse, MANAGED_PROVER_PROTOCOL_VERSION,
        MAX_REQUEST_JSON_BYTES,
    };

    use super::{run_main, FAILURE_MESSAGE};

    fn valid_request() -> ManagedProverRequest {
        ManagedProverRequest {
            protocol_version: MANAGED_PROVER_PROTOCOL_VERSION,
            task_id: "task-prover".into(),
            source: "return input;".into(),
            input: r#"{"value":42}"#.into(),
            max_usage_units: 1_000,
        }
    }

    fn valid_response() -> ManagedProverResponse {
        ManagedProverResponse {
            protocol_version: MANAGED_PROVER_PROTOCOL_VERSION,
            proof_scheme: "test-proof-v1".into(),
            image_id: [1, 2, 3, 4, 5, 6, 7, 8],
            journal: vec![1, 2, 3],
            receipt_json: r#"{"receipt":true}"#.into(),
        }
    }

    #[test]
    fn success_writes_only_newline_free_response_json() {
        let request_bytes = valid_request().to_json_bytes().unwrap();
        let response = valid_response();
        let expected = response.to_json_bytes().unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run_main(
            [OsString::from("managed-proof-prover")],
            request_bytes.as_slice(),
            &mut stdout,
            &mut stderr,
            |_| Ok::<_, &'static str>(response),
        );

        assert_eq!(exit_code, 0);
        assert_eq!(stdout, expected);
        assert!(!stdout.contains(&b'\n'));
        assert!(stderr.is_empty());
    }

    #[test]
    fn extra_arguments_fail_without_invoking_the_prover() {
        let called = Cell::new(false);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run_main(
            [
                OsString::from("managed-proof-prover"),
                OsString::from("unexpected"),
            ],
            valid_request().to_json_bytes().unwrap().as_slice(),
            &mut stdout,
            &mut stderr,
            |_| {
                called.set(true);
                Ok::<_, &'static str>(valid_response())
            },
        );

        assert_eq!(exit_code, 1);
        assert!(!called.get());
        assert!(stdout.is_empty());
        assert_eq!(stderr, FAILURE_MESSAGE);
    }

    #[test]
    fn malformed_or_unknown_request_json_fails_closed() {
        for input in [
            br#"{"protocol_version":1"#.as_slice(),
            br#"{"protocol_version":1,"unexpected":true}"#.as_slice(),
            br#"{} trailing"#.as_slice(),
        ] {
            let called = Cell::new(false);
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();

            let exit_code = run_main(
                [OsString::from("managed-proof-prover")],
                input,
                &mut stdout,
                &mut stderr,
                |_| {
                    called.set(true);
                    Ok::<_, &'static str>(valid_response())
                },
            );

            assert_eq!(exit_code, 1);
            assert!(!called.get());
            assert!(stdout.is_empty());
            assert_eq!(stderr, FAILURE_MESSAGE);
        }
    }

    #[test]
    fn oversized_stdin_fails_before_json_decode_or_proving() {
        let input = vec![b' '; MAX_REQUEST_JSON_BYTES + 1];
        let called = Cell::new(false);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run_main(
            [OsString::from("managed-proof-prover")],
            input.as_slice(),
            &mut stdout,
            &mut stderr,
            |_| {
                called.set(true);
                Ok::<_, &'static str>(valid_response())
            },
        );

        assert_eq!(exit_code, 1);
        assert!(!called.get());
        assert!(stdout.is_empty());
        assert_eq!(stderr, FAILURE_MESSAGE);
    }

    #[test]
    fn prover_and_response_validation_errors_use_the_exact_generic_failure() {
        let request = valid_request().to_json_bytes().unwrap();

        for response in [
            Err::<ManagedProverResponse, _>("prover leaked details"),
            Ok(ManagedProverResponse {
                journal: Vec::new(),
                ..valid_response()
            }),
        ] {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();

            let exit_code = run_main(
                [OsString::from("managed-proof-prover")],
                request.as_slice(),
                &mut stdout,
                &mut stderr,
                |_| response,
            );

            assert_eq!(exit_code, 1);
            assert!(stdout.is_empty());
            assert_eq!(stderr, b"managed proof generation failed\n");
        }
    }
}
