FROM node:22-alpine AS build
WORKDIR /viewer
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM nginx:stable-alpine
ENV WAVEFORM_BACKEND=mchprs:8091
COPY nginx.conf.template /etc/nginx/templates/default.conf.template
COPY --from=build /viewer/dist /usr/share/nginx/html
EXPOSE 8080
