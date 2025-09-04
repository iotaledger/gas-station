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

### Watch Mode (for development)

```bash
npm run watch
```

## API Endpoints

- `GET /` - Health check endpoint
- `GET /aws-kms/get-pubkey-address` - Get the IOTA address for the KMS public key
- `POST /aws-kms/sign-transaction` - Sign a IOTA transaction using KMS

### Sign Transaction Example

```bash
curl -X POST http://localhost:3000/aws-kms/sign-transaction \
  -H "Content-Type: application/json" \
  -d '{"txBytes": "base64-encoded-transaction-bytes"}'
```

## Project Structure

- `index.ts` - Main Express server with API endpoints
- `awsUtils.ts` - AWS KMS integration and IOTA signature utilities
- `package.json` - Project dependencies and scripts
- `tsconfig.json` - TypeScript configuration
