package dag

import (
	"fmt"
	"strings"

	"hivemind/services/nodepool/pb"
)

type TaskSpec struct {
	ID                  string
	ArtifactInputs      []string
	CPUcores            int32
	MemoryGB            int32
	GPUScore            int32
	GPUMemoryGB         int32
	MaxRetries          int32
	DeadlineUnix        int64
	Deterministic       bool
	SideEffects         bool
	Priority            int32
	ExecutionPackageRef string
}

type ExecutionPackageSpec struct {
	RuntimeVersion string
	TaskCodeRef    string
	ArtifactRefs   []string
	Constraints    map[string]string
}

type Graph struct {
	jobID string
	nodes map[string]TaskSpec
	order []string
	edges [][2]string
}

func NewGraph(jobID string) (*Graph, error) {
	jobID = strings.TrimSpace(jobID)
	if jobID == "" {
		return nil, fmt.Errorf("job_id is required")
	}
	return &Graph{
		jobID: jobID,
		nodes: make(map[string]TaskSpec),
	}, nil
}

func (g *Graph) Add(spec TaskSpec) error {
	if g == nil {
		return fmt.Errorf("graph is nil")
	}
	spec.ID = strings.TrimSpace(spec.ID)
	if spec.ID == "" {
		return fmt.Errorf("task id is required")
	}
	if spec.CPUcores <= 0 {
		return fmt.Errorf("cpu cores must be positive")
	}
	if spec.MemoryGB <= 0 {
		return fmt.Errorf("memory_gb must be positive")
	}
	if spec.MaxRetries < 0 {
		return fmt.Errorf("max retries must be non-negative")
	}
	if _, exists := g.nodes[spec.ID]; exists {
		return fmt.Errorf("duplicate task id: %s", spec.ID)
	}
	g.nodes[spec.ID] = spec
	g.order = append(g.order, spec.ID)
	return nil
}

func (g *Graph) DependsOn(beforeTaskID, afterTaskID string) error {
	if g == nil {
		return fmt.Errorf("graph is nil")
	}
	beforeTaskID = strings.TrimSpace(beforeTaskID)
	afterTaskID = strings.TrimSpace(afterTaskID)
	if _, ok := g.nodes[beforeTaskID]; !ok {
		return fmt.Errorf("dependency source task not found: %s", beforeTaskID)
	}
	if _, ok := g.nodes[afterTaskID]; !ok {
		return fmt.Errorf("dependency target task not found: %s", afterTaskID)
	}
	g.edges = append(g.edges, [2]string{beforeTaskID, afterTaskID})
	return nil
}

func (g *Graph) Compile() *pb.DAGIR {
	if g == nil {
		return &pb.DAGIR{}
	}
	nodes := make([]*pb.DAGNode, 0, len(g.order))
	for _, id := range g.order {
		spec := g.nodes[id]
		nodes = append(nodes, &pb.DAGNode{
			TaskId:         spec.ID,
			ArtifactInputs: append([]string(nil), spec.ArtifactInputs...),
			ResourceRequirements: &pb.ResourceRequirements{
				CpuCores:    spec.CPUcores,
				MemoryGb:    spec.MemoryGB,
				GpuScore:    spec.GPUScore,
				GpuMemoryGb: spec.GPUMemoryGB,
			},
			MaxRetries:          spec.MaxRetries,
			DeadlineUnix:        spec.DeadlineUnix,
			Deterministic:       spec.Deterministic,
			SideEffects:         spec.SideEffects,
			Priority:            spec.Priority,
			ExecutionPackageRef: spec.ExecutionPackageRef,
		})
	}
	edges := make([]*pb.DAGEdge, 0, len(g.edges))
	for _, edge := range g.edges {
		edges = append(edges, &pb.DAGEdge{
			FromTaskId: edge[0],
			ToTaskId:   edge[1],
		})
	}
	return &pb.DAGIR{
		JobId: g.jobID,
		Nodes: nodes,
		Edges: edges,
	}
}

func BuildExecutionPackage(spec ExecutionPackageSpec) (*pb.ExecutionPackage, error) {
	spec.RuntimeVersion = strings.TrimSpace(spec.RuntimeVersion)
	spec.TaskCodeRef = strings.TrimSpace(spec.TaskCodeRef)
	if spec.RuntimeVersion == "" {
		return nil, fmt.Errorf("runtime_version is required")
	}
	if spec.TaskCodeRef == "" {
		return nil, fmt.Errorf("task_code_ref is required")
	}
	return &pb.ExecutionPackage{
		RuntimeVersion: spec.RuntimeVersion,
		TaskCodeRef:    spec.TaskCodeRef,
		ArtifactRefs:   append([]string(nil), spec.ArtifactRefs...),
		Constraints:    cloneMap(spec.Constraints),
	}, nil
}

func cloneMap(in map[string]string) map[string]string {
	if len(in) == 0 {
		return nil
	}
	out := make(map[string]string, len(in))
	for key, value := range in {
		out[key] = value
	}
	return out
}
