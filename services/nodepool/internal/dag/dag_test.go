package dag

import "testing"

func TestGraphCompilesPolicyRichDAGIR(t *testing.T) {
	graph, err := NewGraph("job-1")
	if err != nil {
		t.Fatalf("NewGraph: %v", err)
	}
	task := TaskSpec{
		ID:            "transform",
		CPUcores:      2,
		MemoryGB:      4,
		GPUScore:      10,
		GPUMemoryGB:   1,
		MaxRetries:    3,
		DeadlineUnix:  12345,
		Deterministic: true,
		SideEffects:   false,
		Priority:      50,
		ArtifactInputs: []string{
			"artifact://input",
		},
		ExecutionPackageRef: "artifact://package",
	}
	if err := graph.Add(task); err != nil {
		t.Fatalf("Add: %v", err)
	}

	ir := graph.Compile()

	if ir.GetJobId() != "job-1" {
		t.Fatalf("job_id=%q, want job-1", ir.GetJobId())
	}
	if len(ir.GetNodes()) != 1 {
		t.Fatalf("nodes=%d, want 1", len(ir.GetNodes()))
	}
	node := ir.GetNodes()[0]
	if node.GetTaskId() != "transform" {
		t.Fatalf("task_id=%q, want transform", node.GetTaskId())
	}
	if node.GetResourceRequirements().GetCpuCores() != 2 || node.GetResourceRequirements().GetMemoryGb() != 4 {
		t.Fatalf("resource requirements=%+v", node.GetResourceRequirements())
	}
	if node.GetMaxRetries() != 3 || node.GetDeadlineUnix() != 12345 || node.GetPriority() != 50 {
		t.Fatalf("policy fields not compiled: %+v", node)
	}
	if !node.GetDeterministic() || node.GetSideEffects() {
		t.Fatalf("determinism policy not compiled: deterministic=%v side_effects=%v", node.GetDeterministic(), node.GetSideEffects())
	}
	if got := node.GetArtifactInputs(); len(got) != 1 || got[0] != "artifact://input" {
		t.Fatalf("artifact_inputs=%v", got)
	}
	if node.GetExecutionPackageRef() != "artifact://package" {
		t.Fatalf("execution_package_ref=%q", node.GetExecutionPackageRef())
	}
}

func TestGraphCompilesEdges(t *testing.T) {
	graph, err := NewGraph("job-edges")
	if err != nil {
		t.Fatalf("NewGraph: %v", err)
	}
	if err := graph.Add(TaskSpec{ID: "extract", CPUcores: 1, MemoryGB: 1}); err != nil {
		t.Fatalf("Add extract: %v", err)
	}
	if err := graph.Add(TaskSpec{ID: "load", CPUcores: 1, MemoryGB: 1}); err != nil {
		t.Fatalf("Add load: %v", err)
	}
	if err := graph.DependsOn("extract", "load"); err != nil {
		t.Fatalf("DependsOn: %v", err)
	}

	edges := graph.Compile().GetEdges()
	if len(edges) != 1 {
		t.Fatalf("edges=%d, want 1", len(edges))
	}
	if edges[0].GetFromTaskId() != "extract" || edges[0].GetToTaskId() != "load" {
		t.Fatalf("edge=%+v, want extract->load", edges[0])
	}
}

func TestBuildExecutionPackagePinsRuntimeAndArtifacts(t *testing.T) {
	pkg, err := BuildExecutionPackage(ExecutionPackageSpec{
		RuntimeVersion: "rust-worker-v1",
		TaskCodeRef:    "artifact://code",
		ArtifactRefs:   []string{"artifact://input"},
		Constraints:    map[string]string{"os": "linux"},
	})
	if err != nil {
		t.Fatalf("BuildExecutionPackage: %v", err)
	}
	if pkg.GetRuntimeVersion() != "rust-worker-v1" || pkg.GetTaskCodeRef() != "artifact://code" {
		t.Fatalf("package identity=%+v", pkg)
	}
	if got := pkg.GetArtifactRefs(); len(got) != 1 || got[0] != "artifact://input" {
		t.Fatalf("artifact_refs=%v", got)
	}
	if pkg.GetConstraints()["os"] != "linux" {
		t.Fatalf("constraints=%v", pkg.GetConstraints())
	}
}

func TestGraphValidationRejectsInvalidRuntimeShape(t *testing.T) {
	if _, err := NewGraph(""); err == nil {
		t.Fatal("NewGraph accepted empty job id")
	}
	graph, err := NewGraph("job")
	if err != nil {
		t.Fatalf("NewGraph: %v", err)
	}
	if err := graph.Add(TaskSpec{ID: "bad", CPUcores: 0, MemoryGB: 1}); err == nil {
		t.Fatal("Add accepted non-positive CPU")
	}
	if err := graph.Add(TaskSpec{ID: "bad", CPUcores: 1, MemoryGB: 0}); err == nil {
		t.Fatal("Add accepted non-positive memory")
	}
	if err := graph.DependsOn("missing", "other"); err == nil {
		t.Fatal("DependsOn accepted missing tasks")
	}
	if _, err := BuildExecutionPackage(ExecutionPackageSpec{RuntimeVersion: "", TaskCodeRef: "artifact://code"}); err == nil {
		t.Fatal("BuildExecutionPackage accepted empty runtime version")
	}
	if _, err := BuildExecutionPackage(ExecutionPackageSpec{RuntimeVersion: "rust-worker-v1", TaskCodeRef: ""}); err == nil {
		t.Fatal("BuildExecutionPackage accepted empty task code ref")
	}
}
