# KMS Sidecar for IOTA Gas Station

This project provides a KMS (Key Management Service) sidecar for signing transactions sponsored by IOTA Gas Station

## Prerequisites

- Node.js (version 16 or higher)
- npm or yarn
- AWS account with KMS access
- A KMS key configured for ECDSA signing

## Installation

1. Install dependencies:

```bash
npm install
```

## Environment Variables

Create a `.env` file in the project root with the following variables:

```env
# AWS KMS Configuration
AWS_KMS_KEY_ID=your-kms-key-id-here
AWS_REGION=us-east-1
AWS_ACCESS_KEY_ID=your-access-key-id
AWS_SECRET_ACCESS_KEY=your-secret-access-key

# Server Configuration
PORT=3000
```

## Running the Project

```bash
npm run build
npm start
```

## API Endpoints

- `GET /` - Health check endpoint
- `GET /get-pubkey-address` - Get the IOTA address for the KMS public key
- `POST /sign-transaction` - Sign a IOTA transaction using KMS

### Sign Transaction Example

```bash
curl -X POST http://localhost:3000/sign-transaction \
  -H "Content-Type: application/json" \
  -d '{"txBytes": "base64-encoded-transaction-bytes"}'
```
