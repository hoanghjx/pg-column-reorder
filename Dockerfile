FROM node:24-alpine AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY eslint.config.js index.html tsconfig.json vite.config.ts ./
COPY server ./server
COPY src ./src
RUN npm run lint && npm run build

FROM node:24-alpine AS runtime
WORKDIR /app
ENV NODE_ENV=production
COPY package.json package-lock.json ./
RUN npm ci --omit=dev
COPY server ./server
COPY --from=build /app/dist ./dist
EXPOSE 3210
CMD ["npm", "start"]
