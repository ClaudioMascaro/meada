import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ReactFlow, ReactFlowProvider, Handle, Position, MiniMap, MarkerType, useReactFlow, useViewport } from '@xyflow/react';
import '@xyflow/react/dist/base.css';
import './flow.css';

const copy = {
  pt: { zoomIn: 'Ampliar', zoomOut: 'Reduzir', fit: 'Ver tudo', focus: 'Focar etapa', expand: 'Tela cheia', collapse: 'Sair da tela cheia', controls: 'Navegar no diagrama', map: 'Visão do conjunto', instruction: 'Arraste para explorar. Use Tab para percorrer as etapas.', node: 'Etapa de um diagrama de leitura. Tab percorre as etapas; a seleção de etapa centraliza a vista.', edge: 'Relação entre etapas, somente leitura.', delivered: 'Entregue', planned: 'Planejado', guardrail: 'Em caso de falha', specification: 'Especificação', entry: 'Começar', connection: 'Conexão', readonly: 'Somente leitura', reference: 'Ler a referência', previous: 'Etapa anterior', next: 'Próxima etapa' },
  en: { zoomIn: 'Zoom in', zoomOut: 'Zoom out', fit: 'See all', focus: 'Focus a step', expand: 'Full screen', collapse: 'Exit full screen', controls: 'Navigate the diagram', map: 'Overview map', instruction: 'Drag to explore. Use Tab to move through the steps.', node: 'Read-only diagram step. Tab moves between steps; the step selector centers the view.', edge: 'Read-only relationship between steps.', delivered: 'Delivered', planned: 'Planned', guardrail: 'On failure', specification: 'Specification', entry: 'Start', connection: 'Connection', readonly: 'Read only', reference: 'Read the reference', previous: 'Previous step', next: 'Next step' },
  es: { zoomIn: 'Ampliar', zoomOut: 'Reducir', fit: 'Ver todo', focus: 'Enfocar etapa', expand: 'Pantalla completa', collapse: 'Salir de pantalla completa', controls: 'Navegar por el diagrama', map: 'Vista del conjunto', instruction: 'Arrastra para explorar. Usa Tab para recorrer las etapas.', node: 'Etapa de un diagrama de lectura. Tab recorre las etapas; el selector centra la vista.', edge: 'Relación entre etapas, solo lectura.', delivered: 'Entregado', planned: 'Planeado', guardrail: 'En caso de fallo', specification: 'Especificación', entry: 'Comenzar', connection: 'Conexión', readonly: 'Solo lectura', reference: 'Leer la referencia', previous: 'Etapa anterior', next: 'Siguiente etapa' }
};
const sides = { top: Position.Top, right: Position.Right, bottom: Position.Bottom, left: Position.Left };
function Icon({ name }) {
  const paths = {
    plus: <><path d="M5 12h14M12 5v14"/></>, minus: <path d="M5 12h14"/>,
    fit: <><path d="M8 4H4v4m12-4h4v4M4 16v4h4m12-4v4h-4"/><rect x="8" y="8" width="8" height="8" rx="1"/></>,
    expand: <path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/>,
    collapse: <path d="M3 8h5V3m8 0v5h5M8 21v-5H3m13 5v-5h5"/>,
    delivered: <path d="m5 12 4 4L19 6"/>, planned: <><circle cx="12" cy="12" r="8" strokeDasharray="3 3"/><path d="M12 8v4l3 2"/></>,
    guardrail: <><path d="m12 3 10 18H2Z"/><path d="M12 8v6m0 3v.1"/></>,
    process: <><rect x="4" y="5" width="16" height="14" rx="3"/><path d="M8 10h8m-8 4h5"/></>
  };
  return <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}
function Step({ data }) {
  const c = copy[data.locale];
  return <div className={`flow-step flow-step--${data.kind}${data.active ? ' is-active' : ''}`}>
    {Object.entries(sides).flatMap(([side, position]) => [
      <Handle key={`s-${side}`} id={side} type="source" position={position} isConnectable={false}/>,
      <Handle key={`t-${side}`} id={side} type="target" position={position} isConnectable={false}/>
    ])}
    <button type="button" className="flow-step-heading nodrag nopan" aria-pressed={data.active} onClick={() => data.choose(data.id)}><span className="flow-step-icon"><Icon name={data.kind}/></span><strong>{data.title}</strong></button>
    <p>{data.body}</p>
    <div className="flow-step-meta">
      {data.stage && <span className="flow-stage">{data.stage}</span>}
      {data.kind !== 'process' && <span>{c[data.kind]}</span>}
      {data.issue && <a className="nodrag nopan" href={`https://github.com/ClaudioMascaro/meada/issues/${data.issue}`} aria-label={`${c.specification} #${data.issue}`}>#{data.issue}</a>}
    </div>
  </div>;
}
const nodeTypes = { step: Step };

function Toolbar({ graph, locale, shell, choose, selected }) {
  const c = copy[locale];
  const flow = useReactFlow();
  const viewport = useViewport();
  const [full, setFull] = useState(false);
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const duration = reduce ? 0 : 240;
  useEffect(() => {
    const update = () => setFull(document.fullscreenElement === shell.current);
    document.addEventListener('fullscreenchange', update);
    return () => document.removeEventListener('fullscreenchange', update);
  }, [shell]);
  return <div className="flow-toolbar" role="group" aria-label={c.controls}>
    <div className="flow-zoom">
      <button type="button" title={c.zoomOut} aria-label={c.zoomOut} disabled={viewport.zoom <= .25} onClick={() => flow.zoomOut({ duration })}><Icon name="minus"/></button>
      <span aria-live="polite">{Math.round(viewport.zoom * 100)}%</span>
      <button type="button" title={c.zoomIn} aria-label={c.zoomIn} disabled={viewport.zoom >= 1.6} onClick={() => flow.zoomIn({ duration })}><Icon name="plus"/></button>
    </div>
    <button type="button" onClick={() => flow.fitView({ padding: .15, minZoom: .25, maxZoom: 1, duration })}><Icon name="fit"/><span>{c.fit}</span></button>
    <label className="flow-focus"><span className="flow-sr-only">{c.focus}</span><select value={selected} onChange={e => choose(e.target.value)}><option value="" disabled>{c.focus}</option>{graph.nodes.map(n => <option key={n.id} value={n.id}>{n.stage ? `${n.stage} · ` : ''}{n.title}</option>)}</select></label>
    {document.fullscreenEnabled && <button type="button" className="flow-expand" title={full ? c.collapse : c.expand} aria-label={full ? c.collapse : c.expand} onClick={async () => {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await shell.current.requestFullscreen();
    }}><Icon name={full ? 'collapse' : 'expand'}/></button>}
  </div>;
}
function Diagram({ graph, locale, host, wrapper }) {
  const c = copy[locale];
  const shell = useRef(null);
  const flow = useReactFlow();
  const [selected, setSelected] = useState(graph.start);
  const current = graph.nodes.find(n => n.id === selected);
  const currentIndex = graph.nodes.indexOf(current);
  const choose = id => {
    setSelected(id);
    const node = graph.nodes.find(n => n.id === id);
    flow.setCenter(node.position.x + 122, node.position.y + 84, { zoom: 1, duration: matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 240 });
  };
  const nodes = useMemo(() => graph.nodes.map(n => ({ id: n.id, type: 'step', position: n.position, width: 244, height: 168, ariaRole: 'group', data: { ...n, locale, choose, active: n.id === selected }, ariaLabel: `${n.stage || ''} ${n.title}. ${n.body}${n.kind !== 'process' ? `. ${c[n.kind]}` : ''}`, draggable: false, connectable: false, deletable: false })), [graph, locale, c, selected]);
  const edges = useMemo(() => graph.edges.map(e => ({ ...e, type: 'smoothstep', reconnectable: false, deletable: false, animated: false, className: `flow-edge--${e.kind}`, label: e.label, ariaLabel: `${graph.nodes.find(n => n.id === e.source).title} → ${graph.nodes.find(n => n.id === e.target).title}${e.label ? `: ${e.label}` : ''}`, markerEnd: { type: MarkerType.ArrowClosed, color: e.kind === 'failure' ? '#a35a43' : e.kind === 'planned' ? '#927138' : '#587b68' } })), [graph]);
  const start = graph.nodes.find(n => n.id === graph.start);
  return <div className="flow-shell" ref={shell}>
    <div className="flow-heading"><strong>{graph.title}</strong><span>{c.readonly}</span></div>
    <Toolbar graph={graph} locale={locale} shell={shell} choose={choose} selected={selected}/>
    <div className="flow-canvas" role="region" aria-label={graph.title}>
      <ReactFlow nodes={nodes} edges={edges} nodeTypes={nodeTypes}
        nodesDraggable={false} nodesConnectable={false} edgesReconnectable={false} deleteKeyCode={null}
        nodesFocusable edgesFocusable autoPanOnNodeFocus disableKeyboardA11y={false}
        zoomOnScroll={false} panOnScroll={false} preventScrolling={false} zoomOnDoubleClick={false}
        minZoom={.25} maxZoom={1.6} defaultViewport={{ x: start.position.x ? wrapper.clientWidth / 2 - (start.position.x + 122) * .9 : 24, y: 36 - start.position.y * .9, zoom: .9 }}
        attributionPosition="bottom-right" selectionOnDrag={false}
        ariaLabelConfig={{
          'node.a11yDescription.default': c.node, 'node.a11yDescription.keyboardDisabled': c.node,
          'node.a11yDescription.ariaLiveMessage': () => c.readonly, 'edge.a11yDescription.default': c.edge,
          'controls.ariaLabel': c.controls, 'controls.zoomIn.ariaLabel': c.zoomIn, 'controls.zoomOut.ariaLabel': c.zoomOut,
          'controls.fitView.ariaLabel': c.fit, 'controls.interactive.ariaLabel': c.readonly, 'minimap.ariaLabel': c.map, 'handle.ariaLabel': c.connection
        }}
        onInit={() => { wrapper.classList.add('graph-ready'); wrapper.querySelector('details').open = false; host.dataset.ready = 'true'; }}>
        <MiniMap pannable={false} zoomable={false} nodeColor={n => n.data.kind === 'planned' ? '#d6c295' : n.data.kind === 'guardrail' ? '#d8a58f' : '#8fb4a0'} nodeStrokeColor="#4c6e5b" maskColor="rgba(223,232,220,.65)"/>
      </ReactFlow>
    </div>
    <p className="flow-instruction">{c.instruction}</p>
    <div className="flow-inspector">
      <div className="flow-detail" aria-live="polite" aria-atomic="true">
        <strong>{current.stage ? `${current.stage} · ` : ''}{current.title}</strong>
        <p>{current.detail || current.body}</p>
        {current.reference && <a href={current.reference} onClick={() => {
          const target = document.getElementById(decodeURIComponent(current.reference.split('#')[1] || ''));
          const disclosure = target?.closest('details');
          if (disclosure) disclosure.open = true;
        }}>{c.reference}</a>}
      </div>
      <div className="flow-step-navigation">
        <button type="button" onClick={() => choose(graph.nodes[currentIndex - 1].id)} disabled={!currentIndex}>{c.previous}</button>
        <span>{currentIndex + 1} / {graph.nodes.length}</span>
        <button type="button" onClick={() => choose(graph.nodes[currentIndex + 1].id)} disabled={currentIndex === graph.nodes.length - 1}>{c.next}</button>
      </div>
    </div>
  </div>;
}
class GraphBoundary extends React.Component {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  componentDidCatch() { this.props.wrapper.classList.remove('graph-ready'); }
  render() { return this.state.failed ? null : this.props.children; }
}
for (const host of document.querySelectorAll('.meada-flow')) {
  const wrapper = host.closest('.graph-explorer');
  try {
    const data = JSON.parse(wrapper.querySelector('script[type="application/json"]').textContent);
    createRoot(host).render(<GraphBoundary wrapper={wrapper}><ReactFlowProvider><Diagram {...data} host={host} wrapper={wrapper}/></ReactFlowProvider></GraphBoundary>);
  } catch {
    wrapper.classList.remove('graph-ready');
  }
}
