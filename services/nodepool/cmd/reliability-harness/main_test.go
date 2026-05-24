package main

import "testing"

func TestDODSatisfiedRequiresReleaseGateParameters(t *testing.T) {
	summary := summary{
		Parameters: harnessArgs{Runs: 10, FailureSimulations: 3, LongSeconds: 900},
		Runs:       []runResult{{Passed: true}, {Passed: true}},
		Passed:     true,
	}
	if !dodSatisfied(summary) {
		t.Fatal("expected DoD to be satisfied with passing runs and release parameters")
	}

	summary.Parameters.LongSeconds = 899
	if dodSatisfied(summary) {
		t.Fatal("DoD should reject long workload shorter than 900 seconds")
	}
}

func TestWorkloadSpecsIncludeRequiredScenarios(t *testing.T) {
	args := harnessArgs{ParallelTasks: 2}
	specs := workloadSpecs(args)
	seen := map[string]bool{}
	for _, spec := range specs {
		seen[spec.Kind] = true
	}
	for _, required := range []string{"cpu", "io", "failure", "retry", "long", "parallel-0", "parallel-1"} {
		if !seen[required] {
			t.Fatalf("missing workload kind %q in %+v", required, specs)
		}
	}
}
