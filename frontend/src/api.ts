export type DatasetStatus = 'generating' | 'ready' | 'failed'

export type DatasetCreateRequest = {
  dataset_size?: number
}

export type DatasetCreateResponse = {
  dataset_id: string
}

export type DatasetGetResponse = {
  dataset_id: string
  created_at: string
  dataset_size: number
  shard_size: number
  num_buckets: number
  status: DatasetStatus
  shards_total: number
  shards_done: number
  dataset_commitment_hex?: string | null
  error?: string | null
}

export type Metric = 'count' | 'sum' | 'mean'

export type QueryRequest = {
  dataset_id: string
  metric: Metric
  field: 'blood_glucose'
  age_range: { min_age: number; max_age: number }
}

export type QueryResponse = {
  query_id: string
  dataset_id: string
  bucket_index: number
  bucket_range: [number, number]
  sum_glucose: number
  count: number
  mean_glucose?: number | null
  server_verified: boolean
  shard_proofs_endpoint: string
}

const API_KEY = 'dev-secret-key'

async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: {
      'content-type': 'application/json',
      'x-api-key': API_KEY,
      ...(init?.headers ?? {}),
    },
  })

  if (!res.ok) {
    let msg = `${res.status} ${res.statusText}`
    try {
      const body = (await res.json()) as { error?: string }
      if (body?.error) msg = body.error
    } catch {
      // ignore
    }
    throw new Error(msg)
  }

  return (await res.json()) as T
}

export function createDataset(req: DatasetCreateRequest): Promise<DatasetCreateResponse> {
  return fetchJson<DatasetCreateResponse>('/api/v1/datasets', {
    method: 'POST',
    body: JSON.stringify(req),
  })
}

export function getDataset(id: string): Promise<DatasetGetResponse> {
  return fetchJson<DatasetGetResponse>(`/api/v1/datasets/${id}`)
}

export function createQuery(req: QueryRequest): Promise<QueryResponse> {
  return fetchJson<QueryResponse>('/api/v1/queries', {
    method: 'POST',
    body: JSON.stringify(req),
  })
}
