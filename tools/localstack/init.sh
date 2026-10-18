#!/bin/sh
set -eu
export AWS_DEFAULT_REGION=us-east-2

if ! awslocal dynamodb describe-table --table-name math-on-the-go >/dev/null 2>&1; then
  awslocal dynamodb create-table \
    --table-name math-on-the-go \
    --attribute-definitions AttributeName=pk,AttributeType=S AttributeName=sk,AttributeType=S \
    --key-schema AttributeName=pk,KeyType=HASH AttributeName=sk,KeyType=RANGE \
    --billing-mode PAY_PER_REQUEST >/dev/null
fi
awslocal dynamodb wait table-exists --table-name math-on-the-go
