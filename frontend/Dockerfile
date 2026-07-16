FROM node:20-alpine AS builder

WORKDIR /app
COPY package*.json ./
RUN npm ci --include=dev
COPY index.html vite.config.js ./
COPY public ./public
COPY src ./src
ARG VITE_API_BASE=
ENV VITE_API_BASE=$VITE_API_BASE
RUN npm run build

FROM nginx:1.27-alpine
COPY nginx/default.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/dist /usr/share/nginx/html
EXPOSE 80
