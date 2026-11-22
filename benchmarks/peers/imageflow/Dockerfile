FROM mcr.microsoft.com/dotnet/sdk:8.0.428-bookworm-slim@sha256:1b955f179b02df4f0eb5fadedaa898cbe44d620a64aab52eec72c8e91d1355b5 AS build
WORKDIR /src
COPY ImageflowBench.csproj Program.cs ./
RUN dotnet publish -c Release --use-current-runtime --no-self-contained -o /out

FROM mcr.microsoft.com/dotnet/aspnet:8.0.34-bookworm-slim@sha256:dccacae9e0e103b5d3ba5fbe442dd8d3316b990cd8f2ec9be0a0ec9db0c0e520
WORKDIR /app
COPY --from=build /out ./
ENV ASPNETCORE_URLS=http://0.0.0.0:8080 DOTNET_EnableDiagnostics=0
USER app
EXPOSE 8080
ENTRYPOINT ["dotnet", "ImageflowBench.dll"]
