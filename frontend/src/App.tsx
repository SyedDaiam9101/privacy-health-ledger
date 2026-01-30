import { useEffect, useMemo, useState } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import {
  Database,
  ShieldCheck,
  Activity,
  Search,
  RefreshCw,
  AlertCircle,
  BarChart3,
  Layers,
  CheckCircle2
} from 'lucide-react'
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Cell
} from 'recharts'
import './App.css'
import { createDataset, createQuery, getDataset, type DatasetGetResponse, type Metric } from './api'

const AGE_BUCKETS = [
  { label: '0–17', min: 0, max: 17 },
  { label: '18–29', min: 18, max: 29 },
  { label: '30–39', min: 30, max: 39 },
  { label: '40–49', min: 40, max: 49 },
  { label: '50–64', min: 50, max: 64 },
  { label: '65–120', min: 65, max: 120 },
] as const


function App() {
  const [backendError, setBackendError] = useState<string | null>(null)
  const [datasetSize, setDatasetSize] = useState<number>(1_000_000)
  const [datasetId, setDatasetId] = useState<string | null>(null)
  const [dataset, setDataset] = useState<DatasetGetResponse | null>(null)
  const [isCreatingDataset, setIsCreatingDataset] = useState(false)
  const [bucketIndex, setBucketIndex] = useState(2)
  const [metric, setMetric] = useState<Metric>('mean')
  const [isQuerying, setIsQuerying] = useState(false)

  const [queryResult, setQueryResult] = useState<{
    sum: number
    count: number
    mean: number | null
    serverVerified: boolean
    shardProofsEndpoint: string
  } | null>(null)

  const selectedBucket = AGE_BUCKETS[bucketIndex]

  const canQuery = useMemo(() => {
    return dataset?.status === 'ready' && !!datasetId
  }, [dataset?.status, datasetId])

  const progressPercent = useMemo(() => {
    if (!dataset || dataset.shards_total === 0) return 0
    return Math.round((dataset.shards_done / dataset.shards_total) * 100)
  }, [dataset])

  useEffect(() => {
    if (!datasetId) return
    let cancelled = false
    const tick = async () => {
      try {
        const d = await getDataset(datasetId)
        if (!cancelled) {
          setDataset(d)
          setBackendError(null)
        }
      } catch (e) {
        if (!cancelled) setBackendError((e as Error).message)
      }
    }
    void tick()
    const interval = window.setInterval(() => {
      if (dataset?.status === 'generating' || dataset == null) {
        void tick()
      }
    }, 2000)
    return () => {
      cancelled = true
      window.clearInterval(interval)
    }
  }, [datasetId, dataset?.status])

  const onCreateDataset = async () => {
    setBackendError(null)
    setIsCreatingDataset(true)
    setQueryResult(null)
    try {
      const resp = await createDataset({ dataset_size: datasetSize })
      setDatasetId(resp.dataset_id)
      setDataset(null)
    } catch (e) {
      setBackendError((e as Error).message)
    } finally {
      setIsCreatingDataset(false)
    }
  }

  const onRunQuery = async () => {
    if (!datasetId) return
    setBackendError(null)
    setIsQuerying(true)
    try {
      const resp = await createQuery({
        dataset_id: datasetId,
        metric,
        field: 'blood_glucose',
        age_range: { min_age: selectedBucket.min, max_age: selectedBucket.max },
      })
      setQueryResult({
        sum: resp.sum_glucose,
        count: resp.count,
        mean: resp.mean_glucose ?? null,
        serverVerified: resp.server_verified,
        shardProofsEndpoint: resp.shard_proofs_endpoint,
      })
    } catch (e) {
      setBackendError((e as Error).message)
    } finally {
      setIsQuerying(false)
    }
  }

  // Sample data for the chart - in a real app, we'd fetch all buckets to show the distribution
  const chartData = useMemo(() => {
    if (!queryResult) return []
    return AGE_BUCKETS.map((b, i) => ({
      name: b.label,
      value: i === bucketIndex ? (queryResult.mean ?? 0) : 0, // Simplified: only show active query results
      isActive: i === bucketIndex
    }))
  }, [queryResult, bucketIndex])

  return (
    <div className="page animate-fade-in">
      <header className="header">
        <motion.div
          initial={{ scale: 0.9, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          transition={{ duration: 0.5 }}
        >
          <h1>HealthLedger <span style={{ fontWeight: 400, opacity: 0.7 }}>PRO</span></h1>
          <p className="subtitle">
            Privacy-Preserving ZK-Verification for Health Analytics
          </p>
        </motion.div>
      </header>

      <AnimatePresence>
        {backendError && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="error-pill"
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <AlertCircle size={18} />
              <strong>System Error:</strong> {backendError}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <div className="dashboard-grid">
        <div className="column">
          <section className="panel card">
            <h2><Database size={20} /> Data Initialization</h2>
            <div className="control-group" style={{ marginBottom: '1.5rem' }}>
              <label>Target Dataset Scale (synthetic records)</label>
              <div style={{ display: 'flex', gap: '1rem' }}>
                <input
                  type="number"
                  style={{ flex: 1 }}
                  min={1000}
                  step={1000}
                  value={datasetSize}
                  onChange={(e) => setDatasetSize(Number(e.target.value))}
                />
                <button
                  className="btn-primary"
                  onClick={onCreateDataset}
                  disabled={isCreatingDataset}
                >
                  {isCreatingDataset ? <RefreshCw className="spinner" size={18} /> : 'Generate Ledger'}
                </button>
              </div>
            </div>

            {datasetId && (
              <div className="status-box">
                <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '0.5rem' }}>
                  <span className="stat-k">ID: {datasetId.slice(0, 8)}...</span>
                  <span className={`status-badge status-${dataset?.status || 'generating'}`}>
                    {dataset?.status || 'initializing'}
                  </span>
                </div>

                <div className="progress-container">
                  <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '0.75rem', marginBottom: '4px' }}>
                    <span>Proof Generation Phase</span>
                    <span>{progressPercent}%</span>
                  </div>
                  <div className="progress-bar-bg">
                    <motion.div
                      className="progress-bar-fill"
                      initial={{ width: 0 }}
                      animate={{ width: `${progressPercent}%` }}
                    />
                  </div>
                </div>

                {dataset?.dataset_commitment_hex && (
                  <div className="mono" style={{ fontSize: '0.7rem', marginTop: '1rem' }}>
                    <ShieldCheck size={12} style={{ marginRight: '4px' }} />
                    Commitment: {dataset.dataset_commitment_hex.slice(0, 32)}...
                  </div>
                )}
              </div>
            )}
          </section>

          <section className="panel card">
            <h2><Activity size={20} /> Aggregate Query</h2>
            <div className="row" style={{ gap: '1rem' }}>
              <div className="control-group" style={{ flex: 1 }}>
                <label>Metric</label>
                <select value={metric} onChange={(e) => setMetric(e.target.value as Metric)}>
                  <option value="mean">Average (Mean)</option>
                  <option value="sum">Total (Sum)</option>
                  <option value="count">Population (Count)</option>
                </select>
              </div>
              <div className="control-group" style={{ flex: 1 }}>
                <label>Cohort Age Range</label>
                <select value={bucketIndex} onChange={(e) => setBucketIndex(Number(e.target.value))}>
                  {AGE_BUCKETS.map((b, i) => (
                    <option key={b.label} value={i}>{b.label}</option>
                  ))}
                </select>
              </div>
            </div>

            <button
              className="btn-primary"
              style={{ width: '100%', marginTop: '1rem', background: 'var(--secondary)', boxShadow: 'none' }}
              onClick={onRunQuery}
              disabled={!canQuery || isQuerying}
            >
              {isQuerying ? 'Computing Proofs...' : 'Run Verified Query'}
            </button>

            {!canQuery && !datasetId && (
              <p className="stat-k" style={{ marginTop: '1rem', textAlign: 'center' }}>
                Initialize a dataset to begin querying.
              </p>
            )}
          </section>
        </div>

        <div className="column">
          <section className="panel card" style={{ minHeight: '520px' }}>
            <h2><BarChart3 size={20} /> Analysis Results</h2>

            {queryResult ? (
              <motion.div
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                className="query-container"
              >
                <div className="stats-grid">
                  <div className="stat-card">
                    <div className="stat-k">SUM GLUCOSE</div>
                    <div className="stat-v">{queryResult.sum.toLocaleString()}</div>
                  </div>
                  <div className="stat-card">
                    <div className="stat-k">POPULATION</div>
                    <div className="stat-v">{queryResult.count.toLocaleString()}</div>
                  </div>
                  <div className="stat-card" style={{ gridColumn: 'span 2' }}>
                    <div className="stat-k">MEAN BLOOD GLUCOSE (mg/dL)</div>
                    <div className="stat-v" style={{ fontSize: '2rem', color: 'var(--primary)' }}>
                      {queryResult.mean == null ? '—' : queryResult.mean.toFixed(2)}
                    </div>
                  </div>
                </div>

                <div className="chart-container">
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart data={chartData}>
                      <CartesianGrid strokeDasharray="3 3" stroke="#2d2d2d" vertical={false} />
                      <XAxis dataKey="name" stroke="#6b7280" fontSize={12} tickLine={false} axisLine={false} />
                      <YAxis stroke="#6b7280" fontSize={12} tickLine={false} axisLine={false} />
                      <Tooltip
                        contentStyle={{ background: '#111827', border: '1px solid #374151', borderRadius: '8px' }}
                        itemStyle={{ color: '#10b981' }}
                      />
                      <Bar dataKey="value" radius={[4, 4, 0, 0]}>
                        {chartData.map((entry, index) => (
                          <Cell key={`cell-${index}`} fill={entry.isActive ? 'var(--primary)' : '#1f2937'} />
                        ))}
                      </Bar>
                    </BarChart>
                  </ResponsiveContainer>
                </div>

                <div className="verification-status" style={{
                  marginTop: '1.5rem',
                  padding: '1rem',
                  background: queryResult.serverVerified ? 'rgba(16, 185, 129, 0.05)' : 'rgba(239, 68, 68, 0.05)',
                  borderRadius: '12px',
                  border: `1px solid ${queryResult.serverVerified ? 'rgba(16, 185, 129, 0.2)' : 'rgba(239, 68, 68, 0.2)'}`,
                  display: 'flex',
                  alignItems: 'center',
                  gap: '12px'
                }}>
                  {queryResult.serverVerified ? (
                    <CheckCircle2 color="#10b981" size={32} />
                  ) : (
                    <AlertCircle color="#ef4444" size={32} />
                  )}
                  <div>
                    <div style={{ fontWeight: 600, fontSize: '0.9rem' }}>
                      {queryResult.serverVerified ? 'ZK-Proof Verified' : 'Verification Pending'}
                    </div>
                    <div className="stat-k" style={{ fontSize: '0.75rem' }}>
                      {queryResult.serverVerified
                        ? 'The backend has successfully verified Groth16 proofs for all contributing data shards.'
                        : 'Proofs for some data shards are still being computed or failed verification.'}
                    </div>
                  </div>
                </div>
              </motion.div>
            ) : (
              <div style={{
                height: '400px',
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'center',
                color: 'var(--text-muted)'
              }}>
                <Search size={48} strokeWidth={1} style={{ marginBottom: '1rem', opacity: 0.5 }} />
                <p>No active query results to display</p>
              </div>
            )}
          </section>
        </div>
      </div>

      <footer className="footer" style={{ marginTop: '4rem', paddingBottom: '2rem' }}>
        <div className="card" style={{ display: 'flex', alignItems: 'center', gap: '1rem', padding: '1rem 1.5rem' }}>
          <Layers size={20} style={{ color: 'var(--secondary)' }} />
          <div>
            <div style={{ fontSize: '0.85rem', fontWeight: 500 }}>Ledger Architecture</div>
            <p className="stat-k" style={{ fontSize: '0.75rem', marginBottom: 0 }}>
              This system uses shard-level ZK-proofs to ensure data integrity without raw record exposure.
              The private dataset remains on the secure host.
            </p>
          </div>
        </div>
      </footer>
    </div>
  )
}

export default App
