# RPC Log Plugin

The `rpc-log-plugin` records Core Lightning RPC requests as JSON objects in Google Cloud Storage.

It is useful when you need a durable audit trail for debugging, operational analysis, or investigating how a node is
being used. The plugin observes the `rpc_command` hook, replaces values of known sensitive fields with `"***"`, and
uploads each request as a separate object.

Objects are stored under the following prefix:

```text
rpc/<RFC3339 timestamp>-<UUID>.json
```

The UUID makes every object name unique, so the plugin never needs to overwrite an existing log.

## Security model

The plugin recursively redacts named JSON fields whose keys contain one of the configured sensitive terms, including
`rune`, `hsmsecret`, `preimage`, `payment_secret`, `private_key`, `seed`, and `mnemonic`. Matching is
case-insensitive.

This is a defensive filter, not a complete data-loss-prevention system. In particular, positional RPC parameters do
not contain field names and therefore cannot be identified by the current sanitizer. Unknown plugin RPCs may also use
different names for sensitive values. Review the resulting data before enabling the plugin on a production node.

For least privilege, grant the plugin only the `roles/storage.objectCreator` role on its bucket. This allows it to
create objects but not read, list, delete, or overwrite them. Bucket creation must be performed separately by a
deployment or administrator identity.

## Build

From the repository root:

```bash
cargo build --release --package rpc-log-plugin
```

The executable is created at:

```text
target/release/rpc-log-plugin
```

## Create the bucket

The following commands must be run with an identity that can create buckets and manage bucket IAM policies:

```bash
PROJECT_ID="my-project"
BUCKET="my-cln-rpc-logs"
REGION="europe-west1"
WRITER_SA="cln-log-writer@${PROJECT_ID}.iam.gserviceaccount.com"

gcloud services enable storage.googleapis.com \
  --project="${PROJECT_ID}"

gcloud storage buckets create "gs://${BUCKET}" \
  --project="${PROJECT_ID}" \
  --location="${REGION}" \
  --uniform-bucket-level-access

gcloud storage buckets add-iam-policy-binding "gs://${BUCKET}" \
  --member="serviceAccount:${WRITER_SA}" \
  --role="roles/storage.objectCreator"
```

Bucket names are globally unique. Choose a different value if the requested name is already taken.

The plugin accepts the plain bucket ID through `log-bucket`, for example `my-cln-rpc-logs`. Do not pass `gs://` or the
full `projects/_/buckets/...` resource name; the plugin adds the resource-name prefix internally.

## Authentication

The Google Cloud client uses Application Default Credentials (ADC). No credential path is configured in the plugin.

### GKE

Use Workload Identity Federation for GKE and associate the pod's Kubernetes service account with the Google service
account that received `roles/storage.objectCreator`. The pod then obtains short-lived credentials automatically; do
not mount a downloaded service-account key in the container.

### Local development

Authenticate ADC with the Google Cloud CLI:

```bash
gcloud auth application-default login
```

The authenticated development identity must have permission to create objects in the selected bucket.

## Configure Core Lightning

Add the plugin and bucket ID to `lightningd`'s configuration:

```ini
plugin=/absolute/path/to/target/release/rpc-log-plugin
log-bucket=my-cln-rpc-logs
```

Alternatively, pass the same options on the command line:

```bash
lightningd \
  --plugin=/absolute/path/to/target/release/rpc-log-plugin \
  --log-bucket=my-cln-rpc-logs
```

The bucket must already exist when RPC requests are logged. Authentication failures, missing buckets, and upload
errors are returned by the hook, so verify the setup outside production before relying on it.

## Log format

Each uploaded object contains one JSON document:

```json
{
  "method": "pay",
  "request_id": 42,
  "body": {
    "bolt11": "lnbc...",
    "payment_secret": "***"
  },
  "caller": null,
  "peer_id": "02..."
}
```

The request body may still contain financially or personally sensitive information that is not covered by the danger
field list, such as invoices, labels, descriptions, addresses, routes, and payment hashes. Restrict access to the
bucket and configure retention and deletion policies appropriate for your environment.
