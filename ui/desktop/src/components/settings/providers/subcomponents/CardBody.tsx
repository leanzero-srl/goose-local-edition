import React from 'react';

interface CardBodyProps {
  children: React.ReactNode;
}

export default function CardBody({ children }: CardBodyProps) {
  return <div className="flex flex-wrap items-center justify-start gap-2">{children}</div>;
}
