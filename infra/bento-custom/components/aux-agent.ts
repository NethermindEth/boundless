import * as pulumi from "@pulumi/pulumi";
import * as aws from "@pulumi/aws";
import { createEcsTaskRole } from "./iam";

export async function setupAuxAgent(
    name: string,
    network: any,
    database: any,
    cache: any,
    storage: any,
    secrets: any,
    tags: Record<string, string>
) {
    // Get region info
    const region = await aws.getRegion().then(r => r.name);
    
    // Create ECS cluster for Fargate
    const cluster = new aws.ecs.Cluster(`${name}-aux-cluster`, {
        name: `${name}-aux-cluster`,
        settings: [{
            name: "containerInsights",
            value: "enabled",
        }],
        tags: {
            ...tags,
            Name: `${name}-aux-cluster`,
        },
    });

    // Create IAM roles
    const { taskRole, executionRole } = await createEcsTaskRole(`${name}-aux-agent`, tags);

    // Create CloudWatch log group
    const logGroup = new aws.cloudwatch.LogGroup(`${name}-aux-agent-logs`, {
        name: `/aws/ecs/${name}-aux-agent`,
        retentionInDays: 7,
        tags: {
            ...tags,
            Name: `/aws/ecs/${name}-aux-agent`,
        },
    });

    // Create task definition for aux agent
    const taskDefinition = new aws.ecs.TaskDefinition(`${name}-aux-agent-task`, {
        family: `${name}-aux-agent`,
        networkMode: "awsvpc",
        requiresCompatibilities: ["FARGATE"],
        cpu: "1024", // 1 vCPU
        memory: "2048", // 2 GB
        taskRoleArn: taskRole.arn,
        executionRoleArn: executionRole.arn,
        
        containerDefinitions: pulumi.all([
            database.connectionUrl,
            cache.connectionUrl,
            storage.bucketName,
            storage.s3AccessKeyId,
            storage.s3SecretKey,
            logGroup.name,
        ]).apply(([dbUrl, redisUrl, s3Bucket, s3AccessKeyId, s3SecretKey, logGroupName]) => JSON.stringify([{
            name: "aux-agent",
            image: "risczero/risc0-bento-agent:stable",
            command: ["/app/agent", "-t", "aux"],
            essential: true,
            
            environment: [
                { name: "DATABASE_URL", value: dbUrl },
                { name: "REDIS_URL", value: redisUrl },
                { name: "S3_URL", value: "https://s3.amazonaws.com" },
                { name: "S3_BUCKET", value: s3Bucket },
                { name: "AWS_DEFAULT_REGION", value: region },
                { name: "RUST_LOG", value: "info" },
                { name: "LD_LIBRARY_PATH", value: "/usr/local/cuda-12.2/compat/" },
                { name: "RUST_BACKTRACE", value: "1" },
                { name: "S3_ACCESS_KEY", value: s3AccessKeyId },
                { name: "S3_SECRET_KEY", value: s3SecretKey },
            ],
            
            logConfiguration: {
                logDriver: "awslogs",
                options: {
                    "awslogs-group": logGroupName,
                    "awslogs-region": region,
                    "awslogs-stream-prefix": "aux-agent",
                },
            },
            
            healthCheck: {
                command: ["CMD-SHELL", "echo 'healthy'"],
                interval: 30,
                timeout: 5,
                retries: 3,
                startPeriod: 60,
            },
        }])),
        
        tags: {
            ...tags,
            Name: `${name}-aux-agent-task`,
        },
    });

    // Create ECS service
    const service = new aws.ecs.Service(`${name}-aux-agent-service`, {
        name: `${name}-aux-agent`,
        cluster: cluster.id,
        taskDefinition: taskDefinition.arn,
        desiredCount: 1,
        launchType: "FARGATE",
        
        networkConfiguration: {
            subnets: network.privateSubnetIds,
            securityGroups: [network.instanceSecurityGroup.id],
            assignPublicIp: false,
        },
        
        enableEcsManagedTags: true,
        propagateTags: "SERVICE",
        
        tags: {
            ...tags,
            Name: `${name}-aux-agent-service`,
        },
    });

    return {
        cluster,
        task: taskDefinition,
        service,
        logGroup,
    };
}