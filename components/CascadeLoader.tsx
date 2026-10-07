import Image from "next/image";

export function CascadeLoader() {
  return (
    <div className="cascade-loader" data-testid="cascade-loader" aria-hidden="true">
      {Array.from({ length: 4 }, (_, index) => (
        <span className="cascade-loader__bar" key={index}>
          <Image src="/icon.svg" alt="" width={366} height={197} />
        </span>
      ))}
    </div>
  );
}
