FROM alpine:latest

ARG TARGETARCH

COPY ./forsinka-${TARGETARCH} /usr/local/bin/forsinka
RUN chmod +x /usr/local/bin/forsinka

RUN adduser -D forsinka
USER forsinka

ENTRYPOINT ["/usr/local/bin/forsinka"]