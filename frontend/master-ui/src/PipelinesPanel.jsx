import { useMemo, useState } from 'react';
import { Play, Workflow } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { PIPELINE_TEMPLATES, buildDagPipelineGraph } from './pipelineTemplates.mjs';

export function PipelinesPanel({ onLoadTemplateIntoSubmit }) {
  const [selectedId, setSelectedId] = useState(PIPELINE_TEMPLATES[0].id);
  const [simulatedStep, setSimulatedStep] = useState(0);

  const activeTemplate = useMemo(
    () => PIPELINE_TEMPLATES.find((t) => t.id === selectedId) ?? PIPELINE_TEMPLATES[0],
    [selectedId],
  );

  const previewNodes = useMemo(() => {
    return activeTemplate.nodes.map((node, index) => {
      let state = 'pending';
      let billed_cpt = 0;
      if (index < simulatedStep) {
        state = 'completed';
        billed_cpt = Math.max(1, Math.round(node.max_cpt * 0.7));
      } else if (index === simulatedStep && simulatedStep > 0) {
        state = 'running';
      } else if (node.depends_on.length === 0) {
        state = 'ready';
      }
      return { ...node, state, billed_cpt };
    });
  }, [activeTemplate, simulatedStep]);

  const graph = useMemo(
    () => buildDagPipelineGraph(previewNodes, activeTemplate.defaultBudgetCpt),
    [previewNodes, activeTemplate],
  );

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="section-card-header">
          <div>
            <CardTitle className="section-title flex items-center gap-2">
              <Workflow aria-hidden="true" className="size-4" />
              Standard Compute Templates &amp; DAG Pipelines
            </CardTitle>
            <CardDescription>
              Zero-boilerplate templates for ONNX inference, Whisper audio, embeddings, WASM image batches, and Monte Carlo simulations.
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-3 md:grid-cols-3">
            {PIPELINE_TEMPLATES.map((tpl) => {
              const selected = tpl.id === activeTemplate.id;
              return (
                <button
                  key={tpl.id}
                  type="button"
                  onClick={() => {
                    setSelectedId(tpl.id);
                    setSimulatedStep(0);
                  }}
                  className={`rounded-lg border p-3 text-left transition ${
                    selected ? 'border-primary bg-primary/5' : 'border-border hover:bg-muted/50'
                  }`}
                >
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <span className="font-medium text-sm">{tpl.name}</span>
                    <Badge variant="outline">{tpl.runtime}</Badge>
                  </div>
                  <p className="text-xs text-muted-foreground">{tpl.description}</p>
                  <div className="mt-2 text-xs text-muted-foreground">
                    {tpl.nodes.length} nodes · Cap {tpl.defaultBudgetCpt} CPT
                  </div>
                </button>
              );
            })}
          </div>

          <div className="rounded-md border p-3 space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div>
                <h3 className="text-sm font-semibold">{activeTemplate.name} — Visual DAG Stages</h3>
                <p className="text-xs text-muted-foreground">
                  Progress: {graph.progressPct}% ({graph.counts.completed}/{graph.counts.total} completed) · Billed {graph.totalBilledCpt} / {graph.budgetCapCpt} CPT
                </p>
              </div>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => setSimulatedStep((s) => (s + 1) % (activeTemplate.nodes.length + 1))}
                >
                  <Play aria-hidden="true" className="size-3.5" />
                  Preview Step
                </Button>
                {onLoadTemplateIntoSubmit ? (
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => onLoadTemplateIntoSubmit(activeTemplate)}
                  >
                    Use Template
                  </Button>
                ) : null}
              </div>
            </div>

            <div className="grid gap-3 md:grid-cols-3">
              {graph.stages.map((stage) => (
                <div key={stage.stageIndex} className="rounded-md bg-muted/40 p-2 space-y-2">
                  <div className="text-xs font-semibold uppercase text-muted-foreground">
                    Stage {stage.stageIndex + 1}
                  </div>
                  {stage.nodes.map((node) => (
                    <div key={node.id} className="rounded border bg-background p-2 text-xs space-y-1">
                      <div className="flex items-center justify-between">
                        <span className="font-medium">{node.id}</span>
                        <Badge variant={node.state === 'completed' ? 'default' : 'secondary'}>
                          {node.state}
                        </Badge>
                      </div>
                      <div className="text-muted-foreground">
                        kind: {node.kind} · max {node.maxCpt} CPT
                      </div>
                    </div>
                  ))}
                </div>
              ))}
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
