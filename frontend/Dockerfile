FROM node:20-alpine AS builder

WORKDIR /app
COPY package*.json ./
RUN npm ci --include=dev
COPY index.html vite.config.js ./
COPY src ./src
ARG VITE_API_BASE=
ARG VITE_MASTER_UI=http://100.124.230.74:3000
ARG VITE_WORKER_UI=http://100.124.230.74:3001
ENV VITE_API_BASE=$VITE_API_BASE
ENV VITE_MASTER_UI=$VITE_MASTER_UI
ENV VITE_WORKER_UI=$VITE_WORKER_UI
RUN npm run build

FROM nginx:1.27-alpine
COPY nginx/default.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/dist /usr/share/nginx/html
EXPOSE 80
